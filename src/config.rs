use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_site_url")]
    pub site_url: String,
    #[serde(default)]
    pub state_path: Option<PathBuf>,
    #[serde(default = "default_max_items")]
    pub max_items_per_alert: usize,
    #[serde(default)]
    pub ntfy: Ntfy,
    #[serde(default)]
    pub email: Email,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ntfy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ntfy_server")]
    pub server: String,
    #[serde(default)]
    pub topic: String,
    /// Environment variable holding an ntfy access token. Only needed for
    /// protected topics.
    #[serde(default)]
    pub token_env: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Email {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub username: String,
    /// Environment variable holding the SMTP password. The password itself is
    /// never stored in the config file.
    #[serde(default = "default_password_env")]
    pub password_env: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}

fn default_site_url() -> String {
    "https://www.brickborrow.com".to_string()
}

fn default_ntfy_server() -> String {
    "https://ntfy.sh".to_string()
}

fn default_smtp_port() -> u16 {
    587
}

fn default_password_env() -> String {
    "BRICKBORROW_SMTP_PASSWORD".to_string()
}

fn default_max_items() -> usize {
    25
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let config: Config =
            toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.ntfy.enabled || self.email.enabled,
            "no alert channel enabled: set ntfy.enabled or email.enabled"
        );
        if self.ntfy.enabled {
            anyhow::ensure!(!self.ntfy.topic.is_empty(), "ntfy.topic is empty");
        }
        if self.email.enabled {
            anyhow::ensure!(!self.email.smtp_host.is_empty(), "email.smtp_host is empty");
            anyhow::ensure!(!self.email.from.is_empty(), "email.from is empty");
            anyhow::ensure!(!self.email.to.is_empty(), "email.to is empty");
        }
        Ok(())
    }

    /// Where the seen-products snapshot lives, defaulting to
    /// `$XDG_STATE_HOME/brickborrow-watch/state.json`.
    pub fn state_path(&self) -> Result<PathBuf> {
        if let Some(path) = &self.state_path {
            return Ok(path.clone());
        }
        let base = match std::env::var_os("XDG_STATE_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let home = std::env::var_os("HOME").context("HOME is not set")?;
                PathBuf::from(home).join(".local/state")
            }
        };
        Ok(base.join("brickborrow-watch/state.json"))
    }
}
