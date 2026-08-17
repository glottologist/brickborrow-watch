//! Alert delivery. Every enabled channel is attempted; the run counts as
//! delivered if at least one of them succeeds.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::config::{Config, Email, Ntfy};

pub struct Alert {
    pub title: String,
    pub body: String,
}

pub fn send(config: &Config, alert: &Alert) -> Result<()> {
    let mut delivered = 0;
    let mut failures = Vec::new();

    if config.ntfy.enabled {
        match send_ntfy(&config.ntfy, alert) {
            Ok(()) => delivered += 1,
            Err(err) => failures.push(format!("ntfy: {err:#}")),
        }
    }
    if config.email.enabled {
        match send_email(&config.email, alert) {
            Ok(()) => delivered += 1,
            Err(err) => failures.push(format!("email: {err:#}")),
        }
    }

    for failure in &failures {
        eprintln!("alert channel failed: {failure}");
    }
    if delivered == 0 {
        bail!("no alert channel delivered: {}", failures.join("; "));
    }
    Ok(())
}

fn send_ntfy(ntfy: &Ntfy, alert: &Alert) -> Result<()> {
    let url = format!(
        "{}/{}",
        ntfy.server.trim_end_matches('/'),
        ntfy.topic.trim_matches('/')
    );
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let mut request = agent
        .post(&url)
        .set("Title", &ascii_header(&alert.title))
        .set("Tags", "bricks")
        .set("Content-Type", "text/plain; charset=utf-8");

    if let Some(var) = &ntfy.token_env {
        let token =
            std::env::var(var).with_context(|| format!("reading ntfy token from ${var}"))?;
        request = request.set("Authorization", &format!("Bearer {token}"));
    }

    request
        .send_string(&alert.body)
        .with_context(|| format!("posting to {url}"))?;
    Ok(())
}

fn send_email(email: &Email, alert: &Alert) -> Result<()> {
    let password = std::env::var(&email.password_env)
        .with_context(|| format!("reading SMTP password from ${}", email.password_env))?;

    let message = Message::builder()
        .from(email.from.parse().context("parsing email.from")?)
        .to(email.to.parse().context("parsing email.to")?)
        .subject(&alert.title)
        .header(ContentType::TEXT_PLAIN)
        .body(alert.body.clone())
        .context("building email")?;

    let transport = SmtpTransport::starttls_relay(&email.smtp_host)
        .with_context(|| format!("connecting to {}", email.smtp_host))?
        .port(email.smtp_port)
        .credentials(Credentials::new(email.username.clone(), password))
        .timeout(Some(Duration::from_secs(30)))
        .build();

    transport.send(&message).context("sending email")?;
    Ok(())
}

/// ntfy passes headers through as latin-1, so the title stays ASCII and the
/// interesting characters live in the body.
fn ascii_header(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}
