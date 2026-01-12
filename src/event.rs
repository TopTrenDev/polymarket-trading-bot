use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub platform: String,
    pub event_id: String,
    pub title: String,
    pub description: String,
    pub resolution_date: Option<DateTime<Utc>>,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

impl Event {
    pub fn new(
        platform: String,
        event_id: String,
        title: String,
        description: String,
    ) -> Self {
        Self {
            platform,
            event_id,
            title,
            description,
            resolution_date: None,
            category: None,
            tags: Vec::new(),
        }
    }

    pub fn with_resolution_date(mut self, date: DateTime<Utc>) -> Self {
        self.resolution_date = Some(date);
        self
    }

    pub fn with_category(mut self, category: String) -> Self {
        self.category = Some(category);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

#[derive(Debug, Clone)]
pub struct MarketPrices {
    pub yes: f64,
    pub no: f64,
    pub liquidity: f64,
}

impl MarketPrices {
    pub fn new(yes: f64, no: f64, liquidity: f64) -> Self {
        Self {
            yes,
            no,
            liquidity,
        }
    }

    pub fn validate(&self) -> bool {
        // Yes + No should equal ~1.00 (allowing for small rounding)
        (self.yes + self.no - 1.0).abs() < 0.01
    }
}

