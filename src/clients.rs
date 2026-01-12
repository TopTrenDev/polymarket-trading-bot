use crate::event::{Event, MarketPrices};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::{info, warn};

// Polymarket API Client
#[derive(Clone)]
pub struct PolymarketClient {
    http_client: Client,
    polygon_rpc_url: String,
    wallet_private_key: Option<String>,
    base_url: String,
}

impl PolymarketClient {
    pub fn new() -> Self {
        // Create HTTP client with connection pooling and timeouts
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| Client::new()); // Fallback to default if builder fails
        
        Self {
            http_client,
            polygon_rpc_url: std::env::var("POLYGON_RPC_URL")
                .unwrap_or_else(|_| "https://polygon-rpc.com".to_string()),
            wallet_private_key: std::env::var("POLYMARKET_WALLET_PRIVATE_KEY").ok(),
            base_url: "https://gamma-api.polymarket.com".to_string(),
        }
    }

    pub fn with_wallet(mut self, private_key: String) -> Self {
        self.wallet_private_key = Some(private_key);
        self
    }

    pub fn with_rpc(mut self, rpc_url: String) -> Self {
        self.polygon_rpc_url = rpc_url;
        self
    }

    /// Fetch active markets/events from Polymarket
    pub async fn fetch_events(&self) -> Result<Vec<Event>> {
        // Polymarket uses GraphQL API
        let query = r#"
            query GetMarkets($active: Boolean) {
                markets(active: $active, limit: 1000) {
                    id
                    question
                    description
                    endDate
                    category
                    outcomes {
                        title
                        price
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "active": true
        });

        let response = self
            .http_client
            .post(&format!("{}/graphql", self.base_url))
            .json(&serde_json::json!({
                "query": query,
                "variables": variables
            }))
            .send()
            .await
            .context("Failed to fetch Polymarket events")?;

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Polymarket response")?;

        let mut events = Vec::new();

        if let Some(markets) = data["data"]["markets"].as_array() {
            for market in markets {
                let event_id = market["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let title = market["question"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let description = market["description"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let category = market["category"]
                    .as_str()
                    .map(|s| s.to_string());
                
                // Parse end date
                let resolution_date = market["endDate"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                events.push(Event {
                    platform: "polymarket".to_string(),
                    event_id,
                    title,
                    description,
                    resolution_date,
                    category,
                    tags: Vec::new(),
                });
            }
        }

        Ok(events)
    }

    /// Fetch current prices for a market
    pub async fn fetch_prices(&self, event_id: &str) -> Result<MarketPrices> {
        // Use Polymarket's CLOB API for prices
        let url = format!("https://clob.polymarket.com/book", event_id);
        
        let response = self
            .http_client
            .get(&url)
            .query(&[("market", event_id)])
            .send()
            .await
            .context("Failed to fetch Polymarket prices")?;

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse price response")?;

        // Extract Yes and No prices from order book
        let yes_price = data["yes"]
            .as_object()
            .and_then(|o| o.get("bestBid"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let no_price = data["no"]
            .as_object()
            .and_then(|o| o.get("bestBid"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Calculate liquidity (sum of order book depth)
        let liquidity = data["liquidity"]
            .as_f64()
            .unwrap_or(0.0);

        Ok(MarketPrices::new(yes_price, no_price, liquidity))
    }

    /// Place a buy order on Polymarket (requires wallet and blockchain interaction)
    pub async fn place_order(
        &self,
        event_id: String,
        outcome: String, // "YES" or "NO"
        amount: f64,
        max_price: f64,
    ) -> Result<Option<String>> {
        // Check if wallet is configured
        let private_key = self
            .wallet_private_key
            .as_ref()
            .context("Polymarket wallet private key not configured. Set POLYMARKET_WALLET_PRIVATE_KEY environment variable")?;

        // Use blockchain client for order placement
        use crate::polymarket_blockchain::PolymarketBlockchain;
        
        let blockchain = PolymarketBlockchain::new(&self.polygon_rpc_url)?
            .with_wallet(private_key)
            .context("Failed to initialize blockchain client")?;

        // Try blockchain method first, fall back to CLOB if needed
        match blockchain.place_order_via_blockchain(&event_id, &outcome, amount, max_price).await {
            Ok(Some(tx_hash)) => {
                info!("Polymarket order placed via blockchain: {}", tx_hash);
                Ok(Some(tx_hash))
            }
            Ok(None) => {
                warn!("Polymarket order returned None (may need contract addresses)");
                Err(anyhow::anyhow!("Order placement failed - contract addresses may be missing"))
            }
            Err(e) => {
                warn!("Blockchain order failed: {:?}. Attempting CLOB API...", e);
                // Fall back to CLOB API (if implemented)
                blockchain.place_order_via_clob(&self.http_client, &event_id, &outcome, amount, max_price).await
            }
        }
    }

    /// Check if an event is settled and get the outcome
    pub async fn check_settlement(&self, event_id: &str) -> Result<Option<bool>> {
        // Query Polymarket API for market status
        let query = r#"
            query GetMarket($id: ID!) {
                market(id: $id) {
                    resolved
                    outcome
                }
            }
        "#;

        let variables = serde_json::json!({
            "id": event_id
        });

        let response = self
            .http_client
            .post(&format!("{}/graphql", self.base_url))
            .json(&serde_json::json!({
                "query": query,
                "variables": variables
            }))
            .send()
            .await
            .context("Failed to check Polymarket settlement")?;

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse settlement response")?;

        if let Some(resolved) = data["data"]["market"]["resolved"].as_bool() {
            if resolved {
                if let Some(outcome) = data["data"]["market"]["outcome"].as_str() {
                    return Ok(Some(outcome == "YES"));
                }
            }
        }

        Ok(None) // Not yet settled
    }

    /// Get wallet balance (USDC on Polygon)
    pub async fn get_balance(&self) -> Result<f64> {
        let private_key = self
            .wallet_private_key
            .as_ref()
            .context("Wallet private key required for balance check")?;

        // Use blockchain client for balance check
        use crate::polymarket_blockchain::PolymarketBlockchain;
        
        let blockchain = PolymarketBlockchain::new(&self.polygon_rpc_url)?
            .with_wallet(private_key)
            .context("Failed to initialize blockchain client")?;

        blockchain.get_usdc_balance().await
    }
}

// Kalshi API Client
#[derive(Clone)]
pub struct KalshiClient {
    http_client: Client,
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl KalshiClient {
    pub fn new(api_key: String, api_secret: String) -> Self {
        // Create HTTP client with connection pooling and timeouts
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| Client::new()); // Fallback to default if builder fails
        
        Self {
            http_client,
            api_key,
            api_secret,
            base_url: "https://api.cfexchange.com".to_string(), // Kalshi API base URL
        }
    }

    /// Generate authentication headers for Kalshi API
    /// Uses RSA-PSS signature for secure authentication
    fn get_auth_headers(&self, method: &str, path: &str, body: &str) -> Result<reqwest::header::HeaderMap> {
        use reqwest::header::{HeaderMap, HeaderValue};
        use std::time::{SystemTime, UNIX_EPOCH};
        use rsa::{RsaPrivateKey, pkcs1v15::{SigningKey, VerifyingKey}};
        use rsa::signature::{Signer, Verifier};
        use sha2::Sha256;
        use base64::{engine::general_purpose, Engine as _};

        let mut headers = HeaderMap::new();
        
        // Kalshi uses timestamp-based authentication
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();

        // Create signature string: timestamp\nmethod\npath\nbody
        let signature_string = format!("{}\n{}\n{}\n{}", timestamp, method, path, body);

        // Try to parse API secret as RSA private key (PEM format)
        // Kalshi API secret might be in different formats, so we try multiple
        let signature_b64 = if let Ok(private_key) = RsaPrivateKey::from_pkcs8_pem(&self.api_secret) {
            // PEM format - create signing key
            let signing_key = SigningKey::<Sha256>::new(private_key);
            
            // Sign the message
            let signature = signing_key.sign(signature_string.as_bytes());
            
            // Encode signature in Base64
            general_purpose::STANDARD.encode(&signature.to_bytes())
        } else if let Ok(private_key) = RsaPrivateKey::from_pkcs1_pem(&self.api_secret) {
            // Try PKCS1 format
            let signing_key = SigningKey::<Sha256>::new(private_key);
            let signature = signing_key.sign(signature_string.as_bytes());
            general_purpose::STANDARD.encode(&signature.to_bytes())
        } else {
            // If RSA parsing fails, fall back to API key only
            // Some endpoints may work with just API key
            warn!("Failed to parse RSA private key from API secret. Using API key only authentication.");
            String::new()
        };

        // Add headers
        headers.insert(
            "X-API-KEY",
            HeaderValue::from_str(&self.api_key)
                .context("Invalid API key")?,
        );
        
        headers.insert(
            "X-TIMESTAMP",
            HeaderValue::from_str(&timestamp)
                .context("Invalid timestamp")?,
        );
        
        if !signature_b64.is_empty() {
            headers.insert(
                "X-SIGNATURE",
                HeaderValue::from_str(&signature_b64)
                    .context("Invalid signature")?,
            );
        }
        
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/json"),
        );

        Ok(headers)
    }

    /// Fetch active events from Kalshi
    pub async fn fetch_events(&self) -> Result<Vec<Event>> {
        let path = "/trade-api/v2/events";
        let headers = self.get_auth_headers("GET", path, "")?;

        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .headers(headers)
            .query(&[("status", "open"), ("limit", "1000")])
            .send()
            .await
            .context("Failed to fetch Kalshi events")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Kalshi API error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Kalshi response")?;

        let mut events = Vec::new();

        if let Some(events_array) = data["events"].as_array() {
            for event_data in events_array {
                let event_ticker = event_data["event_ticker"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let title = event_data["title"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let subtitle = event_data["subtitle"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let category = event_data["category"]
                    .as_str()
                    .map(|s| s.to_string());

                // Parse expiration time
                let resolution_date = event_data["expected_expiration_time"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                events.push(Event {
                    platform: "kalshi".to_string(),
                    event_id: event_ticker,
                    title,
                    description: subtitle,
                    resolution_date,
                    category,
                    tags: Vec::new(),
                });
            }
        }

        Ok(events)
    }

    /// Fetch current prices for a Kalshi event
    pub async fn fetch_prices(&self, event_id: &str) -> Result<MarketPrices> {
        let path = format!("/trade-api/v2/events/{}/markets", event_id);
        let headers = self.get_auth_headers("GET", &path, "")?;

        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .headers(headers)
            .send()
            .await
            .context("Failed to fetch Kalshi prices")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Kalshi API error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Kalshi price response")?;

        let mut yes_price = 0.0;
        let mut no_price = 0.0;
        let mut liquidity = 0.0;

        if let Some(markets) = data["markets"].as_array() {
            for market in markets {
                let subtitle = market["subtitle"].as_str().unwrap_or("");
                let last_price = market["last_price"]
                    .as_i64()
                    .unwrap_or(0) as f64
                    / 100.0; // Kalshi uses cents, convert to dollars

                if subtitle == "Yes" {
                    yes_price = last_price;
                } else if subtitle == "No" {
                    no_price = last_price;
                }

                if let Some(vol) = market["volume"].as_f64() {
                    liquidity += vol;
                }
            }
        }

        Ok(MarketPrices::new(yes_price, no_price, liquidity))
    }

    /// Place a buy order on Kalshi
    pub async fn place_order(
        &self,
        event_id: String,
        outcome: String, // "YES" or "NO"
        amount: f64,
        price: f64,
    ) -> Result<Option<String>> {
        let path = "/trade-api/v2/orders";
        
        // Kalshi order format
        let order_data = serde_json::json!({
            "event_ticker": event_id,
            "side": "buy",
            "outcome": outcome,
            "count": (amount / price) as i64, // Number of shares
            "price": (price * 100) as i64,    // Kalshi uses cents
        });

        let body = serde_json::to_string(&order_data)?;
        let headers = self.get_auth_headers("POST", path, &body)?;

        let response = self
            .http_client
            .post(&format!("{}{}", self.base_url, path))
            .headers(headers)
            .json(&order_data)
            .send()
            .await
            .context("Failed to place Kalshi order")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Kalshi order failed: {} - {}",
                response.status(),
                error_text
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Kalshi order response")?;

        let order_id = data["order"]["order_id"]
            .as_str()
            .map(|s| s.to_string());

        Ok(order_id)
    }

    /// Check if an event is settled and get the outcome
    pub async fn check_settlement(&self, event_id: &str) -> Result<Option<bool>> {
        let path = format!("/trade-api/v2/events/{}", event_id);
        let headers = self.get_auth_headers("GET", &path, "")?;

        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .headers(headers)
            .send()
            .await
            .context("Failed to check Kalshi settlement")?;

        if !response.status().is_success() {
            return Ok(None); // Event might not exist or not accessible
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse settlement response")?;

        // Check if event is resolved
        if let Some(status) = data["event"]["status"].as_str() {
            if status == "resolved" {
                // Get outcome
                if let Some(outcome) = data["event"]["outcome"].as_str() {
                    return Ok(Some(outcome == "Yes" || outcome == "YES"));
                }
            }
        }

        Ok(None) // Not yet settled
    }

    /// Get account balance
    pub async fn get_balance(&self) -> Result<f64> {
        let path = "/trade-api/v2/portfolio/balance";
        let headers = self.get_auth_headers("GET", path, "")?;

        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .headers(headers)
            .send()
            .await
            .context("Failed to fetch Kalshi balance")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Kalshi balance check failed: {}",
                response.status()
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse balance response")?;

        let balance = data["balance"]
            .as_f64()
            .or_else(|| data["balance"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0.0);

        Ok(balance)
    }
}
