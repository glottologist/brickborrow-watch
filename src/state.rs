//! Snapshot of everything seen so far, and the diff that drives alerts.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::wix::Product;

const STATE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub updated: String,
    pub products: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub in_stock: bool,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A product id the watcher has never recorded before.
    New,
    /// A known product that went from out of stock to in stock.
    BackInStock,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: Kind,
    pub product: Product,
}

impl State {
    pub fn empty() -> Self {
        Self {
            version: STATE_VERSION,
            updated: String::new(),
            products: BTreeMap::new(),
        }
    }

    /// Returns `None` when no snapshot exists yet, which marks a seeding run.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        let state: State =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        anyhow::ensure!(
            state.version == STATE_VERSION,
            "state file version {} is not supported (expected {STATE_VERSION})",
            state.version
        );
        Ok(Some(state))
    }

    pub fn diff(&self, products: &[Product]) -> Vec<Event> {
        products
            .iter()
            .filter_map(|product| {
                let kind = match self.products.get(&product.id) {
                    None => Kind::New,
                    Some(seen) if product.in_stock && !seen.in_stock => Kind::BackInStock,
                    Some(_) => return None,
                };
                Some(Event {
                    kind,
                    product: product.clone(),
                })
            })
            .collect()
    }

    /// Records the current catalogue. Products absent from `products` are kept
    /// so that a temporarily delisted set does not re-alert as brand new.
    pub fn record(&mut self, products: &[Product], now: &str) {
        for product in products {
            self.products
                .entry(product.id.clone())
                .and_modify(|entry| {
                    entry.name = product.name.clone();
                    entry.in_stock = product.in_stock;
                    entry.last_seen = now.to_string();
                })
                .or_insert_with(|| Entry {
                    name: product.name.clone(),
                    in_stock: product.in_stock,
                    first_seen: now.to_string(),
                    last_seen: now.to_string(),
                });
        }
        self.updated = now.to_string();
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(id: &str, in_stock: bool) -> Product {
        Product {
            id: id.to_string(),
            name: format!("LEGO® ({id}) Test Set"),
            in_stock,
            ribbon: None,
            page_url: Some(format!("/product-page/{id}")),
            price: None,
        }
    }

    fn seeded(products: &[Product]) -> State {
        let mut state = State::empty();
        state.record(products, "now");
        state
    }

    #[test]
    fn unseen_products_are_new_whatever_their_stock() {
        let state = State::empty();
        let events = state.diff(&[product("10307", false), product("10281", true)]);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event.kind == Kind::New));
    }

    #[test]
    fn restock_is_reported_once() {
        let mut state = seeded(&[product("10307", false)]);
        let restocked = [product("10307", true)];

        let events = state.diff(&restocked);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::BackInStock);

        state.record(&restocked, "later");
        assert!(state.diff(&restocked).is_empty());
    }

    #[test]
    fn going_out_of_stock_is_not_an_event() {
        let state = seeded(&[product("10307", true)]);
        assert!(state.diff(&[product("10307", false)]).is_empty());
    }

    #[test]
    fn delisted_then_relisted_does_not_alert_as_new() {
        let mut state = seeded(&[product("10307", false)]);
        state.record(&[], "later");
        assert!(state.diff(&[product("10307", false)]).is_empty());
    }
}
