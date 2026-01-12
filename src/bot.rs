use crate::arbitrage_detector::{ArbitrageDetector, ArbitrageOpportunity};
use crate::event::{Event, MarketPrices};
use crate::event_matcher::EventMatcher;
use chrono::{DateTime, Duration, Utc};
use std::time::Duration as StdDuration;
use tokio::time;

pub struct MarketFilters {
    pub categories: Vec<String>,
    pub max_hours_until_resolution: i64,
    pub min_liquidity: f64,
}

impl Default for MarketFilters {
    fn default() -> Self {
        Self {
            categories: vec!["crypto".to_string(), "sports".to_string()],
            max_hours_until_resolution: 24,
            min_liquidity: 100.0,
        }
    }
}

pub struct ShortTermArbitrageBot {
    filters: MarketFilters,
    event_matcher: EventMatcher,
    arbitrage_detector: ArbitrageDetector,
}

impl ShortTermArbitrageBot {
    pub fn new(
        filters: MarketFilters,
        similarity_threshold: f64,
        min_profit_threshold: f64,
    ) -> Self {
        Self {
            filters,
            event_matcher: EventMatcher::new(similarity_threshold),
            arbitrage_detector: ArbitrageDetector::new(min_profit_threshold),
        }
    }

    pub fn is_within_timeframe(&self, resolution_date: Option<DateTime<Utc>>) -> bool {
        if let Some(date) = resolution_date {
            let now = Utc::now();
            let time_until_resolution = date - now;
            let max_time = Duration::hours(self.filters.max_hours_until_resolution);
            let min_time = Duration::minutes(5);

            time_until_resolution >= min_time && time_until_resolution <= max_time
        } else {
            false
        }
    }

    pub fn matches_category(&self, event: &Event) -> bool {
        if self.filters.categories.is_empty() {
            return true;
        }

        let event_category = event.category.as_ref().map(|s| s.to_lowercase()).unwrap_or_default();
        let event_title = event.title.to_lowercase();
        let event_desc = event.description.to_lowercase();

        // Check category field
        for cat in &self.filters.categories {
            if event_category.contains(&cat.to_lowercase()) {
                return true;
            }
        }

        // Check title/description for crypto keywords
        let crypto_keywords = [
            "bitcoin", "btc", "ethereum", "eth", "crypto", "cryptocurrency",
            "price", "above", "below", "reach", "hit", "surpass",
        ];

        // Check title/description for sports keywords
        let sports_keywords = [
            "game", "match", "team", "player", "score", "win", "lose",
            "nfl", "nba", "mlb", "soccer", "football", "basketball",
        ];

        let text = event_title + " " + &event_desc;

        if self.filters.categories.iter().any(|c| c == "crypto") {
            if crypto_keywords.iter().any(|kw| text.contains(kw)) {
                return true;
            }
        }

        if self.filters.categories.iter().any(|c| c == "sports") {
            if sports_keywords.iter().any(|kw| text.contains(kw)) {
                return true;
            }
        }

        false
    }

    pub fn filter_events(&self, events: &[Event]) -> Vec<Event> {
        events
            .iter()
            .filter(|event| {
                self.matches_category(event) && self.is_within_timeframe(event.resolution_date)
            })
            .cloned()
            .collect()
    }

    pub async fn scan_for_opportunities<F, Fut>(
        &self,
        pm_events: &[Event],
        kalshi_events: &[Event],
        fetch_prices: F,
    ) -> Vec<(Event, Event, ArbitrageOpportunity)>
    where
        F: Fn(&str, &str) -> Fut,
        Fut: std::future::Future<Output = MarketPrices> + Send,
    {
        // Filter events
        let pm_filtered = self.filter_events(pm_events);
        let kalshi_filtered = self.filter_events(kalshi_events);

        if pm_filtered.is_empty() || kalshi_filtered.is_empty() {
            return Vec::new();
        }

        // Match events
        let matches = self.event_matcher.find_matches(&pm_filtered, &kalshi_filtered);

        if matches.is_empty() {
            return Vec::new();
        }

        // Check arbitrage for each matched pair
        let mut opportunities = Vec::new();

        for (pm_event, kalshi_event, similarity) in matches {
            // Fetch prices (placeholder - replace with actual API calls)
            let pm_prices = fetch_prices(&pm_event.event_id, "polymarket").await;
            let kalshi_prices = fetch_prices(&kalshi_event.event_id, "kalshi").await;

            // Check liquidity
            if pm_prices.liquidity < self.filters.min_liquidity
                || kalshi_prices.liquidity < self.filters.min_liquidity
            {
                continue;
            }

            // Check arbitrage
            if let Some(opportunity) = self.arbitrage_detector.check_arbitrage(&pm_prices, &kalshi_prices) {
                opportunities.push((pm_event, kalshi_event, opportunity));
            }
        }

        opportunities
    }

    pub async fn run_continuous<F, Fut, P, PFut>(
        &self,
        scan_interval: StdDuration,
        fetch_events: F,
        fetch_prices: P,
    ) -> Vec<(Event, Event, ArbitrageOpportunity)>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = (Vec<Event>, Vec<Event>)> + Send,
        P: Fn(&str, &str) -> PFut + Clone + Send + Sync,
        PFut: std::future::Future<Output = MarketPrices> + Send,
    {
        let mut interval = time::interval(scan_interval);

        loop {
            interval.tick().await;

            let (pm_events, kalshi_events) = fetch_events().await;
            let opportunities = self.scan_for_opportunities(&pm_events, &kalshi_events, fetch_prices.clone()).await;

            if !opportunities.is_empty() {
                tracing::info!("Found {} arbitrage opportunities", opportunities.len());
                for (pm_event, kalshi_event, opp) in &opportunities {
                    tracing::info!(
                        "Opportunity: {} - Profit: ${:.4}, ROI: {:.2}%",
                        pm_event.title,
                        opp.net_profit,
                        opp.roi_percent
                    );
                }
                return opportunities; // Return opportunities for execution
            }
        }
    }
}

