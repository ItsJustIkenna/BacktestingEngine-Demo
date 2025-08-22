use config::BacktestConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BacktestConfig::default();
    
    // Save to sample.yaml
    config.to_file("sample_config.yaml")?;
    
    println!("Sample config saved to sample_config.yaml");
    
    // Also print to console
    let yaml_str = serde_yaml::to_string(&config)?;
    println!("\nSample YAML configuration:");
    println!("{}", yaml_str);
    
    Ok(())
}
