use crate::event::MarketPrices;

#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub strategy: String,
    pub kalshi_action: (String, String, f64), // (action, outcome, price)
    pub polymarket_action: (String, String, f64),
    pub total_cost: f64,
    pub gross_profit: f64,
    pub fees: f64,
    pub net_profit: f64,
    pub roi_percent: f64,
}

pub struct ArbitrageDetector {
    min_profit_threshold: f64,
    fees: Fees,
}

#[derive(Debug, Clone)]
pub struct Fees {
    pub polymarket: f64,
    pub kalshi: f64,
}

impl Default for Fees {
    fn default() -> Self {
        Self {
            polymarket: 0.01, // 1%
            kalshi: 0.01,     // 1%
        }
    }
}

impl ArbitrageDetector {
    pub fn new(min_profit_threshold: f64) -> Self {
        Self {
            min_profit_threshold,
            fees: Fees::default(),
        }
    }

    pub fn with_fees(mut self, fees: Fees) -> Self {
        self.fees = fees;
        self
    }

    pub fn check_arbitrage(
        &self,
        pm_prices: &MarketPrices,
        kalshi_prices: &MarketPrices,
    ) -> Option<ArbitrageOpportunity> {
        // Strategy 1: Buy Yes on Kalshi + Buy No on Polymarket
        let cost_strategy_1 = kalshi_prices.yes + pm_prices.no;
        let profit_strategy_1 = 1.0 - cost_strategy_1;

        // Strategy 2: Buy No on Kalshi + Buy Yes on Polymarket
        let cost_strategy_2 = kalshi_prices.no + pm_prices.yes;
        let profit_strategy_2 = 1.0 - cost_strategy_2;

        // Account for fees
        let total_fees = self.fees.polymarket + self.fees.kalshi;

        // Check Strategy 1
        if profit_strategy_1 > total_fees + self.min_profit_threshold {
            return Some(ArbitrageOpportunity {
                strategy: "Buy Yes on Kalshi + Buy No on Polymarket".to_string(),
                kalshi_action: ("BUY".to_string(), "YES".to_string(), kalshi_prices.yes),
                polymarket_action: ("BUY".to_string(), "NO".to_string(), pm_prices.no),
                total_cost: cost_strategy_1,
                gross_profit: profit_strategy_1,
                fees: total_fees,
                net_profit: profit_strategy_1 - total_fees,
                roi_percent: ((profit_strategy_1 - total_fees) / cost_strategy_1) * 100.0,
            });
        }

        // Check Strategy 2
        if profit_strategy_2 > total_fees + self.min_profit_threshold {
            return Some(ArbitrageOpportunity {
                strategy: "Buy No on Kalshi + Buy Yes on Polymarket".to_string(),
                kalshi_action: ("BUY".to_string(), "NO".to_string(), kalshi_prices.no),
                polymarket_action: ("BUY".to_string(), "YES".to_string(), pm_prices.yes),
                total_cost: cost_strategy_2,
                gross_profit: profit_strategy_2,
                fees: total_fees,
                net_profit: profit_strategy_2 - total_fees,
                roi_percent: ((profit_strategy_2 - total_fees) / cost_strategy_2) * 100.0,
            });
        }

        None
    }
}

