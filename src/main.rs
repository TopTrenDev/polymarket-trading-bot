use anyhow::Result;
use polymarket_kalshi_arbitrage_bot::{
    bot::{MarketFilters, ShortTermArbitrageBot},
    clients::{KalshiClient, PolymarketClient},
    event::MarketPrices,
    position_tracker::PositionTracker,
    settlement_checker::SettlementChecker,
    trade_executor::TradeExecutor,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn, Level};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Starting Polymarket-Kalshi Arbitrage Bot");

    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize clients with required credentials
    let polygon_rpc = std::env::var("POLYGON_RPC_URL")
        .unwrap_or_else(|_| "https://polygon-rpc.com".to_string());
    let wallet_key = std::env::var("POLYMARKET_WALLET_PRIVATE_KEY")
        .ok();
    
    let mut polymarket_client = PolymarketClient::new()
        .with_rpc(polygon_rpc);
    
    if let Some(key) = wallet_key {
        polymarket_client = polymarket_client.with_wallet(key);
    } else {
        warn!("⚠️ POLYMARKET_WALLET_PRIVATE_KEY not set - trading will fail!");
    }

    let kalshi_api_key = std::env::var("KALSHI_API_KEY")
        .unwrap_or_else(|_| {
            warn!("⚠️ KALSHI_API_KEY not set - Kalshi API calls will fail!");
            "".to_string()
        });
    let kalshi_api_secret = std::env::var("KALSHI_API_SECRET")
        .unwrap_or_else(|_| {
            warn!("⚠️ KALSHI_API_SECRET not set - Kalshi API calls will fail!");
            "".to_string()
        });
    
    if kalshi_api_key.is_empty() || kalshi_api_secret.is_empty() {
        error!("❌ Kalshi API credentials missing! Set KALSHI_API_KEY and KALSHI_API_SECRET");
        return Err(anyhow::anyhow!("Missing Kalshi API credentials"));
    }
    
    let kalshi_client = KalshiClient::new(kalshi_api_key, kalshi_api_secret);

    // Wrap clients in Arc for sharing
    let polymarket_client = Arc::new(polymarket_client);
    let kalshi_client = Arc::new(kalshi_client);

    // Create position tracker
    let position_tracker = Arc::new(Mutex::new(PositionTracker::new()));

    // Create trade executor with position tracker
    let trade_executor = Arc::new(
        TradeExecutor::new(
            (*polymarket_client.clone()).clone(),
            (*kalshi_client.clone()).clone(),
        )
        .with_position_tracker(position_tracker.clone()),
    );

    // Create settlement checker
    let settlement_checker = Arc::new(SettlementChecker::new(
        polymarket_client.clone(),
        kalshi_client.clone(),
        position_tracker.clone(),
    ));

    // Configure filters
    let filters = MarketFilters {
        categories: vec!["crypto".to_string(), "sports".to_string()],
        max_hours_until_resolution: 24,
        min_liquidity: 100.0,
    };

    // Create bot
    let bot = ShortTermArbitrageBot::new(
        filters,
        0.80, // similarity threshold
        0.02, // min profit threshold (2%)
    );

    // Fetch prices function
    let fetch_prices = {
        let pm = polymarket_client.clone();
        let kalshi = kalshi_client.clone();
        move |event_id: &str, platform: &str| {
            let event_id = event_id.to_string();
            let platform = platform.to_string();
            let pm = pm.clone();
            let kalshi = kalshi.clone();
            async move {
                match platform.as_str() {
                    "polymarket" => pm.fetch_prices(&event_id).await.unwrap_or_default(),
                    "kalshi" => kalshi.fetch_prices(&event_id).await.unwrap_or_default(),
                    _ => MarketPrices::new(0.0, 0.0, 0.0),
                }
            }
        }
    };

    // Run continuous scanning (every 60 seconds)
    info!("Starting continuous scanning (interval: 60s)");
    info!("Settlement checking (every 5 minutes)");
    
    let mut scan_interval = tokio::time::interval(Duration::from_secs(60));
    let mut settlement_interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
    
    loop {
        tokio::select! {
            _ = scan_interval.tick() => {
        
        // Fetch events
        let (pm_events, kalshi_events) = tokio::join!(
            polymarket_client.fetch_events(),
            kalshi_client.fetch_events()
        );
        
        let pm_events = pm_events.unwrap_or_default();
        let kalshi_events = kalshi_events.unwrap_or_default();
        
        // Scan for opportunities
        let opportunities = bot.scan_for_opportunities(&pm_events, &kalshi_events, fetch_prices.clone()).await;
        
        // Execute trades for found opportunities
        if !opportunities.is_empty() {
            info!("Found {} arbitrage opportunities", opportunities.len());
            
            for (pm_event, kalshi_event, opp) in opportunities {
                info!(
                    "🚨 Arbitrage Opportunity: {} - Profit: ${:.4}, ROI: {:.2}%",
                    pm_event.title,
                    opp.net_profit,
                    opp.roi_percent
                );

                // Execute trade (with default amount - you may want to make this configurable)
                let trade_amount = 100.0; // $100 default
                
                match trade_executor
                    .execute_arbitrage(&opp, &pm_event, &kalshi_event, trade_amount)
                    .await
                {
                    Ok(result) => {
                        if result.success {
                            info!(
                                "✅ Trade executed successfully! PM Order: {:?}, Kalshi Order: {:?}",
                                result.polymarket_order_id, result.kalshi_order_id
                            );
                        } else {
                            info!(
                                "⚠️ Trade execution failed: {}",
                                result.error.unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => {
                        error!("Error executing trade: {}", e);
                    }
                }
            }
            }
            _ = settlement_interval.tick() => {
                // Check for settlements
                info!("Checking for settled positions...");
                match settlement_checker.check_settlements().await {
                    Ok(count) => {
                        if count > 0 {
                            info!("✅ {} positions settled!", count);
                            
                            // Show statistics
                            let stats = settlement_checker.get_statistics().await;
                            info!(
                                "📊 Statistics - Total: {}, Open: {}, Won: {}, Lost: {}, Total Profit: ${:.2}",
                                stats.total_positions,
                                stats.open_positions,
                                stats.won_positions,
                                stats.lost_positions,
                                stats.total_profit
                            );
                            
                            // Check balances
                            if let Ok((pm_balance, kalshi_balance)) = settlement_checker.check_balances().await {
                                info!(
                                    "💰 Current Balances - Polymarket: ${:.2}, Kalshi: ${:.2}, Total: ${:.2}",
                                    pm_balance,
                                    kalshi_balance,
                                    pm_balance + kalshi_balance
                                );
                            }
                        } else {
                            info!("No new settlements");
                        }
                    }
                    Err(e) => {
                        error!("Error checking settlements: {}", e);
                    }
                }
            }
        }
    }
}
