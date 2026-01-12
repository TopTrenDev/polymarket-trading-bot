use crate::clients::{KalshiClient, PolymarketClient};
use crate::position_tracker::{Position, PositionStatus, PositionTracker};
use anyhow::Result;
use std::sync::Arc;
use tracing::{info, warn};

pub struct SettlementChecker {
    polymarket_client: Arc<PolymarketClient>,
    kalshi_client: Arc<KalshiClient>,
    position_tracker: Arc<tokio::sync::Mutex<PositionTracker>>,
}

impl SettlementChecker {
    pub fn new(
        polymarket_client: Arc<PolymarketClient>,
        kalshi_client: Arc<KalshiClient>,
        position_tracker: Arc<tokio::sync::Mutex<PositionTracker>>,
    ) -> Self {
        Self {
            polymarket_client,
            kalshi_client,
            position_tracker,
        }
    }

    /// Check all open positions for settlement
    pub async fn check_settlements(&self) -> Result<usize> {
        let mut settled_count = 0;
        let tracker = self.position_tracker.lock().await;
        let open_positions = tracker.get_open_positions();
        drop(tracker); // Release lock before async operations

        for position in open_positions {
            let position_id = position.id.clone();
            let event_id = position.event_id.clone();
            let outcome = position.outcome.clone();
            let platform = position.platform.clone();

            // Check settlement based on platform
            let settlement_result = match platform.as_str() {
                "polymarket" => {
                    self.polymarket_client.check_settlement(&event_id).await
                }
                "kalshi" => {
                    self.kalshi_client.check_settlement(&event_id).await
                }
                _ => Ok(None),
            };

            match settlement_result {
                Ok(Some(resolved_yes)) => {
                    // Event is settled!
                    let won = (resolved_yes && outcome == "YES") 
                        || (!resolved_yes && outcome == "NO");

                    let payout = if won {
                        Some(position.amount * 1.0) // $1.00 per token/share
                    } else {
                        Some(0.0) // Lost
                    };

                    // Update position
                    let mut tracker = self.position_tracker.lock().await;
                    if let Some(profit) = tracker.update_position_settlement(
                        &position_id,
                        won,
                        payout,
                    ) {
                        settled_count += 1;
                        info!(
                            "✅ Position settled: {} - {} - Profit: ${:.2}",
                            position.event_title,
                            if won { "WON" } else { "LOST" },
                            profit
                        );
                    }
                }
                Ok(None) => {
                    // Event not yet settled, continue waiting
                }
                Err(e) => {
                    warn!("Error checking settlement for {}: {}", event_id, e);
                }
            }
        }

        Ok(settled_count)
    }

    /// Check balances on both platforms
    pub async fn check_balances(&self) -> Result<(f64, f64)> {
        let (pm_balance, kalshi_balance) = tokio::join!(
            self.polymarket_client.get_balance(),
            self.kalshi_client.get_balance()
        );

        let pm_balance = pm_balance.unwrap_or(0.0);
        let kalshi_balance = kalshi_balance.unwrap_or(0.0);

        info!(
            "💰 Balances - Polymarket: ${:.2}, Kalshi: ${:.2}, Total: ${:.2}",
            pm_balance,
            kalshi_balance,
            pm_balance + kalshi_balance
        );

        Ok((pm_balance, kalshi_balance))
    }

    /// Get position statistics
    pub async fn get_statistics(&self) -> crate::position_tracker::PositionStatistics {
        let tracker = self.position_tracker.lock().await;
        tracker.get_statistics()
    }
}

