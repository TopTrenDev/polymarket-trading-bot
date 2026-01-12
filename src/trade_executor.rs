use crate::arbitrage_detector::ArbitrageOpportunity;
use crate::clients::{KalshiClient, PolymarketClient};
use crate::event::Event;
use crate::position_tracker::{Position, PositionTracker};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct TradeResult {
    pub success: bool,
    pub polymarket_order_id: Option<String>,
    pub kalshi_order_id: Option<String>,
    pub error: Option<String>,
}

pub struct TradeExecutor {
    polymarket_client: PolymarketClient,
    kalshi_client: KalshiClient,
    position_tracker: Option<Arc<Mutex<PositionTracker>>>,
}

impl TradeExecutor {
    pub fn new(polymarket_client: PolymarketClient, kalshi_client: KalshiClient) -> Self {
        Self {
            polymarket_client,
            kalshi_client,
            position_tracker: None,
        }
    }

    pub fn with_position_tracker(mut self, tracker: Arc<Mutex<PositionTracker>>) -> Self {
        self.position_tracker = Some(tracker);
        self
    }

    /// Execute arbitrage trade on both platforms simultaneously
    pub async fn execute_arbitrage(
        &self,
        opportunity: &ArbitrageOpportunity,
        pm_event: &Event,
        kalshi_event: &Event,
        amount: f64,
    ) -> Result<TradeResult> {
        info!(
            "Executing arbitrage: {} - Expected profit: ${:.4} ({:.2}% ROI)",
            opportunity.strategy, opportunity.net_profit, opportunity.roi_percent
        );

        // Execute trades simultaneously on both platforms
        let (pm_result, kalshi_result) = tokio::join!(
            self.execute_polymarket_trade(
                pm_event,
                &opportunity.polymarket_action,
                amount
            ),
            self.execute_kalshi_trade(
                kalshi_event,
                &opportunity.kalshi_action,
                amount
            )
        );

        let pm_success = pm_result.is_ok();
        let kalshi_success = kalshi_result.is_ok();

        // Check if both trades succeeded
        if pm_success && kalshi_success {
            info!(
                "✅ Arbitrage executed successfully! PM: {:?}, Kalshi: {:?}",
                pm_result.as_ref().unwrap(),
                kalshi_result.as_ref().unwrap()
            );

            let pm_order_id = pm_result.unwrap();
            let kalshi_order_id = kalshi_result.unwrap();

            // Track positions if tracker is available
            if let Some(tracker) = &self.position_tracker {
                let mut tracker = tracker.lock().await;
                
                // Track Polymarket position
                let pm_position = Position::new(
                    "polymarket".to_string(),
                    pm_event,
                    opportunity.polymarket_action.1.clone(), // outcome
                    amount / opportunity.polymarket_action.2, // amount / price
                    amount * opportunity.polymarket_action.2, // cost
                    opportunity.polymarket_action.2, // price
                    pm_order_id.clone(),
                );
                tracker.add_position(pm_position);

                // Track Kalshi position
                let kalshi_position = Position::new(
                    "kalshi".to_string(),
                    kalshi_event,
                    opportunity.kalshi_action.1.clone(), // outcome
                    amount / opportunity.kalshi_action.2, // amount / price
                    amount * opportunity.kalshi_action.2, // cost
                    opportunity.kalshi_action.2, // price
                    kalshi_order_id.clone(),
                );
                tracker.add_position(kalshi_position);
            }

            Ok(TradeResult {
                success: true,
                polymarket_order_id: pm_order_id,
                kalshi_order_id: kalshi_order_id,
                error: None,
            })
        } else {
            // One or both trades failed
            let mut errors = Vec::new();
            if let Err(e) = pm_result {
                errors.push(format!("Polymarket: {}", e));
            }
            if let Err(e) = kalshi_result {
                errors.push(format!("Kalshi: {}", e));
            }

            let error_msg = errors.join("; ");

            warn!("⚠️ Arbitrage execution failed: {}", error_msg);

            // If one succeeded, we need to cancel it (or handle partial execution)
            if pm_success {
                warn!("Polymarket trade succeeded but Kalshi failed - may need to cancel PM trade");
            }
            if kalshi_success {
                warn!("Kalshi trade succeeded but Polymarket failed - may need to cancel Kalshi trade");
            }

            Ok(TradeResult {
                success: false,
                polymarket_order_id: pm_result.ok().flatten(),
                kalshi_order_id: kalshi_result.ok().flatten(),
                error: Some(error_msg),
            })
        }
    }

    /// Execute trade on Polymarket
    async fn execute_polymarket_trade(
        &self,
        event: &Event,
        action: &(String, String, f64), // (action, outcome, price)
        amount: f64,
    ) -> Result<Option<String>> {
        let (action_type, outcome, max_price) = action;

        info!(
            "Placing {} order on Polymarket: {} {} @ ${:.4} (amount: ${:.2})",
            action_type, outcome, max_price, amount
        );

        // Execute actual Polymarket trade
        match self
            .polymarket_client
            .place_order(
                event.event_id.clone(),
                outcome.clone(),
                amount,
                *max_price,
            )
            .await
        {
            Ok(order_id) => order_id,
            Err(e) => {
                error!("Polymarket order failed: {}", e);
                return Err(e);
            }
        }
        
        info!("✅ Polymarket order placed: {}", order_id);
        Ok(Some(order_id))
    }

    /// Execute trade on Kalshi
    async fn execute_kalshi_trade(
        &self,
        event: &Event,
        action: &(String, String, f64), // (action, outcome, price)
        amount: f64,
    ) -> Result<Option<String>> {
        let (action_type, outcome, price) = action;

        info!(
            "Placing {} order on Kalshi: {} {} @ ${:.4} (amount: ${:.2})",
            action_type, outcome, price, amount
        );

        // Execute actual Kalshi trade
        match self
            .kalshi_client
            .place_order(
                event.event_id.clone(),
                outcome.clone(),
                amount,
                *price,
            )
            .await
        {
            Ok(order_id) => order_id,
            Err(e) => {
                error!("Kalshi order failed: {}", e);
                return Err(e);
            }
        }
        
        info!("✅ Kalshi order placed: {}", order_id);
        Ok(Some(order_id))
    }

    /// Cancel an order (if needed due to partial execution)
    pub async fn cancel_order(&self, platform: &str, order_id: &str) -> Result<()> {
        match platform {
            "polymarket" => {
                // TODO: Implement Polymarket order cancellation
                info!("Cancelling Polymarket order: {}", order_id);
                Ok(())
            }
            "kalshi" => {
                // TODO: Implement Kalshi order cancellation
                info!("Cancelling Kalshi order: {}", order_id);
                Ok(())
            }
            _ => {
                error!("Unknown platform: {}", platform);
                Err(anyhow::anyhow!("Unknown platform: {}", platform))
            }
        }
    }

    /// Get order status
    pub async fn get_order_status(&self, platform: &str, order_id: &str) -> Result<String> {
        match platform {
            "polymarket" => {
                // TODO: Implement Polymarket order status check
                Ok("filled".to_string())
            }
            "kalshi" => {
                // TODO: Implement Kalshi order status check
                Ok("filled".to_string())
            }
            _ => Err(anyhow::anyhow!("Unknown platform: {}", platform)),
        }
    }
}

