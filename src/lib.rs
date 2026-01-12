// Core modules
pub mod event;
pub mod event_matcher;
pub mod arbitrage_detector;
pub mod bot;
pub mod clients;
pub mod trade_executor;
pub mod position_tracker;
pub mod settlement_checker;
pub mod polymarket_blockchain;

// Re-exports
pub use event::{Event, MarketPrices};
pub use event_matcher::EventMatcher;
pub use arbitrage_detector::{ArbitrageDetector, ArbitrageOpportunity};
pub use bot::{ShortTermArbitrageBot, MarketFilters};
pub use clients::{PolymarketClient, KalshiClient};
pub use trade_executor::{TradeExecutor, TradeResult};
pub use position_tracker::{PositionTracker, Position, PositionStatus, PositionStatistics};
pub use settlement_checker::SettlementChecker;

