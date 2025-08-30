//! Development Optimization Examples
//! 
//! Demonstrates the dramatic performance improvements achieved through
//! intelligent caching and parallel processing.

use development::*;

/// Example: Traditional vs Optimized Development Workflow
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🚀 BacktestingEngine Development Optimization Demo");
    println!("==================================================\n");

    // Initialize the development system
    init().await?;
    
    // Run the built-in demo
    execute_dev_command(DevCommand::Demo).await?;
    println!();
    
    // Show individual commands
    demo_individual_commands().await?;
    
    Ok(())
}

/// Demonstrate individual development commands
async fn demo_individual_commands() -> anyhow::Result<()> {
    println!("� Individual Development Commands");
    println!("----------------------------------");
    
    println!("1. Smart Build:");
    execute_dev_command(DevCommand::Build { 
        strategy: "demo_strategy.rs".to_string(),
        force: false 
    }).await?;
    println!();
    
    println!("2. Quick Test:");
    execute_dev_command(DevCommand::QuickTest { 
        strategy: "cached_demo_strategy".to_string(),
        sample_days: 7 
    }).await?;
    println!();
    
    println!("3. Cache Statistics:");
    execute_dev_command(DevCommand::CacheStats).await?;
    
    Ok(())
}
