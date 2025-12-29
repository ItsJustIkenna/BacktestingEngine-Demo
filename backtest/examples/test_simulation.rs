//! Quick test of realistic simulation with synthetic L3 data
//! 
//! Run with: cargo run --release -p backtest --example test_simulation

use backtest::realistic_simulation::{RealisticSimulator, RealisticASParams, OrderBookEvent, EventType, Side};
use rand::prelude::*;

fn main() {
    println!("🚀 Realistic Simulation Test");
    println!("============================\n");
    
    // Generate synthetic L3 order book data that simulates realistic market behavior
    let events = generate_synthetic_l3_data(100_000);
    
    println!("📊 Generated {} synthetic L3 events", events.len());
    
    // Show sample of data
    println!("\n📋 Sample events:");
    for (i, e) in events.iter().take(5).enumerate() {
        println!("   [{}] ts={}, type={:?}, side={:?}, price={:.4}, qty={:.4}",
            i, e.timestamp_ms, e.event_type, e.side, e.price, e.quantity);
    }
    
    // Count event types
    let mut new_count = 0;
    let mut modify_count = 0;
    let mut cancel_count = 0;
    let mut trade_count = 0;
    for e in &events {
        match e.event_type {
            EventType::New => new_count += 1,
            EventType::Modify => modify_count += 1,
            EventType::Cancel => cancel_count += 1,
            EventType::Trade => trade_count += 1,
        }
    }
    println!("\n📈 Event breakdown:");
    println!("   New: {}, Modify: {}, Cancel: {}, Trade: {}", 
        new_count, modify_count, cancel_count, trade_count);
    
    // Run simulation with default AS params
    println!("\n🎯 Running simulation...");
    
    let params = RealisticASParams::default();
    let initial_capital = 10000.0;
    
    let simulator = RealisticSimulator::new(initial_capital, 0.0002); // 2 bps maker fee
    let result = simulator.run(&events, &params);
    
    // Display results
    println!("\n📊 RESULTS");
    println!("==========");
    println!("Total trades: {}", result.num_trades);
    println!("Total volume: ${:.2}", result.total_volume);
    println!("Total fees: ${:.2}", result.total_fees);
    println!("Total PnL: ${:.2}", result.total_pnl);
    println!("Win rate: {:.1}%", result.win_rate * 100.0);
    println!("Sharpe ratio: {:.4}", result.sharpe_ratio);
    println!("Max drawdown: {:.2}%", result.max_drawdown * 100.0);
    println!("Profit factor: {:.2}", result.profit_factor);
    
    if result.num_trades == 0 {
        println!("\n⚠️  WARNING: Zero trades executed!");
        println!("   Check debug output above for NO_CROSS messages.");
        println!("   This means trades aren't crossing our quotes.");
    } else {
        println!("\n✅ Simulation completed with {} trades!", result.num_trades);
    }
    
    // Equity curve sample
    println!("\nEquity curve ({} points):", result.equity_curve.len());
    if !result.equity_curve.is_empty() {
        println!("   First: ${:.2}", result.equity_curve.first().unwrap());
        println!("   Last:  ${:.2}", result.equity_curve.last().unwrap());
        
        // Find min/max
        let min = result.equity_curve.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = result.equity_curve.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        println!("   Min:   ${:.2}", min);
        println!("   Max:   ${:.2}", max);
    }
}

/// Generate synthetic L3 order book data that simulates a realistic market
fn generate_synthetic_l3_data(num_events: usize) -> Vec<OrderBookEvent> {
    let mut rng = rand::thread_rng();
    let mut events = Vec::with_capacity(num_events);
    
    // Start price around $1.65 (like SUI)
    let mut mid_price = 1.65;
    let mut timestamp_ms: i64 = 1700000000000; // Nov 2023
    
    // Track active orders for cancels
    let mut active_bids: Vec<(String, f64, f64)> = Vec::new(); // (order_id, price, qty)
    let mut active_asks: Vec<(String, f64, f64)> = Vec::new();
    let mut order_counter = 0u64;
    
    for _ in 0..num_events {
        // Random time increment (10-500ms)
        timestamp_ms += rng.gen_range(10..500);
        
        // Random walk for mid price (small movements)
        mid_price *= 1.0 + rng.gen_range(-0.0005..0.0005);
        
        // Determine event type
        let event_type_roll: f64 = rng.gen();
        
        let event = if event_type_roll < 0.40 {
            // 40% New orders
            order_counter += 1;
            let order_id = format!("order_{}", order_counter);
            let is_bid = rng.gen_bool(0.5);
            let side = if is_bid { Side::Buy } else { Side::Sell };
            
            // Price near mid with some spread
            let offset = rng.gen_range(0.0001..0.005) * mid_price;
            let price = if is_bid {
                mid_price - offset
            } else {
                mid_price + offset
            };
            let quantity = rng.gen_range(10.0..500.0);
            
            // Track active order
            if is_bid {
                active_bids.push((order_id.clone(), price, quantity));
            } else {
                active_asks.push((order_id.clone(), price, quantity));
            }
            
            OrderBookEvent {
                timestamp_ms,
                event_type: EventType::New,
                order_id,
                side,
                price,
                quantity,
            }
        } else if event_type_roll < 0.70 {
            // 30% Cancels
            let use_bid = rng.gen_bool(0.5);
            let orders = if use_bid { &mut active_bids } else { &mut active_asks };
            
            if !orders.is_empty() {
                let idx = rng.gen_range(0..orders.len());
                let (order_id, price, quantity) = orders.remove(idx);
                OrderBookEvent {
                    timestamp_ms,
                    event_type: EventType::Cancel,
                    order_id,
                    side: if use_bid { Side::Buy } else { Side::Sell },
                    price,
                    quantity,
                }
            } else {
                // No orders to cancel, create a new one
                order_counter += 1;
                OrderBookEvent {
                    timestamp_ms,
                    event_type: EventType::New,
                    order_id: format!("order_{}", order_counter),
                    side: Side::Buy,
                    price: mid_price - 0.001,
                    quantity: 50.0,
                }
            }
        } else {
            // 30% Trades
            // Trades happen AT or NEAR the current mid price
            let is_sell_aggressor = rng.gen_bool(0.5);
            let trade_price = mid_price * (1.0 + rng.gen_range(-0.0002..0.0002));
            let quantity = rng.gen_range(5.0..100.0);
            
            order_counter += 1;
            OrderBookEvent {
                timestamp_ms,
                event_type: EventType::Trade,
                order_id: format!("trade_{}", order_counter),
                side: if is_sell_aggressor { Side::Sell } else { Side::Buy },
                price: trade_price,
                quantity,
            }
        };
        
        events.push(event);
    }
    
    events
}
