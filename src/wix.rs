//! Reads the Brick Borrow catalogue through the Wix Data query API.
//!
//! The `/setsonstock` page is members-only, but the collection behind it
//! (`Stores/Products`) is readable by site visitors, so an anonymous token from
//! `/_api/v1/access-tokens` is enough. No account credentials are involved.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

/// App definition id of the Wix Data Binding app, whose instance token
/// authorises visitor-level collection reads.
const DATA_BINDING_APP_ID: &str = "225dd912-7dea-4738-8688-4b8c6955ffc2";
const COLLECTION: &str = "Stores/Products";
/// The API rejects any page larger than this.
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 200;
const USER_AGENT: &str = concat!("brickborrow-watch/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Deserialize)]
pub struct Product {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(rename = "inStock")]
    pub in_stock: bool,
    #[serde(default)]
    pub ribbon: Option<String>,
    #[serde(rename = "productPageUrl", default)]
    pub page_url: Option<String>,
    #[serde(rename = "formattedPrice", default)]
    pub price: Option<String>,
}

impl Product {
    pub fn url(&self, site_url: &str) -> String {
        match &self.page_url {
            Some(path) if path.starts_with('/') => {
                format!("{}{}", site_url.trim_end_matches('/'), path)
            }
            Some(other) => other.clone(),
            None => site_url.to_string(),
        }
    }
}

#[derive(Deserialize)]
struct QueryResponse {
    items: Vec<Product>,
    #[serde(rename = "totalCount")]
    total_count: usize,
}

pub struct Client {
    site_url: String,
    agent: ureq::Agent,
    token: String,
}

impl Client {
    pub fn connect(site_url: &str) -> Result<Self> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .user_agent(USER_AGENT)
            .build();
        let token = fetch_token(&agent, site_url)?;
        Ok(Self {
            site_url: site_url.trim_end_matches('/').to_string(),
            agent,
            token,
        })
    }

    /// Fetches every product in the catalogue.
    ///
    /// Returns an error rather than a short list if paging does not add up, so
    /// a truncated read can never be mistaken for sets disappearing.
    pub fn fetch_all(&self) -> Result<Vec<Product>> {
        let mut products: Vec<Product> = Vec::new();
        let mut expected: Option<usize> = None;

        for page in 0..MAX_PAGES {
            let offset = page * PAGE_SIZE;
            let response = self.query(offset)?;
            let total = *expected.get_or_insert(response.total_count);
            if response.total_count != total {
                bail!(
                    "catalogue changed size mid-read ({total} then {}), retrying next run",
                    response.total_count
                );
            }
            if response.items.is_empty() {
                break;
            }
            products.extend(response.items);
            if products.len() >= total {
                break;
            }
        }

        let expected = expected.context("no pages returned")?;
        if products.len() != expected {
            bail!("read {} of {expected} products", products.len());
        }
        Ok(products)
    }

    fn query(&self, offset: usize) -> Result<QueryResponse> {
        let body = json!({
            "collectionName": COLLECTION,
            "segment": "LIVE",
            "query": {
                "paging": { "offset": offset, "limit": PAGE_SIZE },
                "fields": [
                    "name",
                    "inStock",
                    "ribbon",
                    "productPageUrl",
                    "formattedPrice",
                ],
            },
        });
        self.agent
            .post(&format!(
                "{}/_api/cloud-data/v1/wix-data/collections/query",
                self.site_url
            ))
            .set("Authorization", &self.token)
            .set("Content-Type", "application/json")
            .send_json(body)
            .with_context(|| format!("querying products at offset {offset}"))?
            .into_json()
            .context("parsing product page")
    }
}

fn fetch_token(agent: &ureq::Agent, site_url: &str) -> Result<String> {
    let tokens: serde_json::Value = agent
        .get(&format!(
            "{}/_api/v1/access-tokens",
            site_url.trim_end_matches('/')
        ))
        .call()
        .context("fetching visitor access tokens")?
        .into_json()
        .context("parsing visitor access tokens")?;

    tokens["apps"][DATA_BINDING_APP_ID]["instance"]
        .as_str()
        .map(str::to_string)
        .context("no Data Binding instance token in access-tokens response")
}
