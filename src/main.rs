mod config;
mod notify;
mod state;
mod wix;

use std::path::PathBuf;

use anyhow::{Context, Result};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::config::Config;
use crate::notify::Alert;
use crate::state::{Event, Kind, State};
use crate::wix::Product;

const USAGE: &str = "\
brickborrow-watch — alerts when Brick Borrow sets become available

Usage: brickborrow-watch [--config PATH] [--dry-run | --test-alert]

  --config PATH  config file (default: $XDG_CONFIG_HOME/brickborrow-watch/config.toml)
  --dry-run      print the alert instead of sending it, and leave state untouched
  --test-alert   send a sample alert through every configured channel and exit
  -h, --help     show this help
";

struct Args {
    config: Option<PathBuf>,
    dry_run: bool,
    test_alert: bool,
}

fn main() -> Result<()> {
    let args = match parse_args()? {
        Some(args) => args,
        None => {
            print!("{USAGE}");
            return Ok(());
        }
    };

    let config_path = match args.config {
        Some(path) => path,
        None => default_config_path()?,
    };
    let config = Config::load(&config_path)?;

    if args.test_alert {
        let alert = Alert {
            title: "Brick Borrow: test alert".to_string(),
            body: "[BACK IN STOCK] LEGO® (10281) Botanicals™ Bonsai Tree\n\
                   https://www.brickborrow.com/product-page/lego-10281-botanicals-bonsai-tree\n\
                   \nThis is a test; no set changed hands.\n"
                .to_string(),
        };
        if args.dry_run {
            println!("--- {}\n{}", alert.title, alert.body);
            return Ok(());
        }
        notify::send(&config, &alert)?;
        println!("test alert delivered");
        return Ok(());
    }

    let state_path = config.state_path()?;

    let client = wix::Client::connect(&config.site_url)?;
    let products = client.fetch_all()?;
    let in_stock = products.iter().filter(|p| p.in_stock).count();
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;

    let mut state = match State::load(&state_path)? {
        Some(state) => state,
        None => {
            let mut seeded = State::empty();
            seeded.record(&products, &now);
            if args.dry_run {
                println!(
                    "first run (dry): would record {} products ({in_stock} in stock), no alerts",
                    products.len()
                );
                return Ok(());
            }
            seeded.save(&state_path)?;
            println!(
                "first run: recorded {} products ({in_stock} in stock) to {}, no alerts sent",
                products.len(),
                state_path.display()
            );
            return Ok(());
        }
    };

    let events = state.diff(&products);
    println!(
        "{now}: {} products, {in_stock} in stock, {} events",
        products.len(),
        events.len()
    );

    if !events.is_empty() {
        let alert = build_alert(&config, &events);
        if args.dry_run {
            println!("--- {}\n{}", alert.title, alert.body);
            return Ok(());
        }
        // Deliver before recording, so a failed alert is retried next run
        // rather than silently swallowed.
        notify::send(&config, &alert)?;
    }

    if !args.dry_run {
        state.record(&products, &now);
        state.save(&state_path)?;
    }
    Ok(())
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        config: None,
        dry_run: false,
        test_alert: false,
    };
    let mut raw = std::env::args().skip(1);
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--dry-run" => args.dry_run = true,
            "--test-alert" => args.test_alert = true,
            "--config" => {
                let path = raw.next().context("--config needs a path")?;
                args.config = Some(PathBuf::from(path));
            }
            other => anyhow::bail!("unknown argument {other}\n\n{USAGE}"),
        }
    }
    Ok(Some(args))
}

fn default_config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var_os("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("brickborrow-watch/config.toml"))
}

fn build_alert(config: &Config, events: &[Event]) -> Alert {
    let mut sorted: Vec<&Event> = events.iter().collect();
    sorted.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.product.name.cmp(&b.product.name))
    });

    let available = sorted.iter().filter(|e| e.product.in_stock).count();
    let title = if available == events.len() {
        format!("Brick Borrow: {} set(s) available", events.len())
    } else {
        format!(
            "Brick Borrow: {} set(s) available, {} listed",
            available,
            events.len() - available
        )
    };

    let shown = sorted.len().min(config.max_items_per_alert);
    let mut body = String::new();
    for event in &sorted[..shown] {
        body.push_str(&line(&config.site_url, event));
    }
    if sorted.len() > shown {
        body.push_str(&format!("... and {} more\n", sorted.len() - shown));
    }
    Alert { title, body }
}

fn is_priced(price: &str) -> bool {
    price.chars().any(|c| c.is_ascii_digit() && c != '0')
}

fn rank(kind: Kind) -> u8 {
    match kind {
        Kind::BackInStock => 0,
        Kind::New => 1,
    }
}

fn line(site_url: &str, event: &Event) -> String {
    let Product {
        name,
        in_stock,
        price,
        ribbon,
        ..
    } = &event.product;
    let tag = match (event.kind, in_stock) {
        (Kind::BackInStock, _) => "BACK IN STOCK",
        (Kind::New, true) => "NEW, AVAILABLE",
        (Kind::New, false) => "NEW, OUT OF STOCK",
    };
    let mut detail = Vec::new();
    // Borrowable sets are all listed at £0.00, so a zero price says nothing.
    if let Some(price) = price.as_deref().filter(|p| is_priced(p)) {
        detail.push(price.to_string());
    }
    if let Some(ribbon) = ribbon.as_deref().filter(|r| !r.is_empty()) {
        detail.push(ribbon.to_string());
    }
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(" ({})", detail.join(", "))
    };
    format!(
        "[{tag}] {name}{detail}\n{}\n\n",
        event.product.url(site_url)
    )
}
