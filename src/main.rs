use std::env;
use std::fs::File;
use std::io::Read;
use std::time::Duration;
use serde::{Deserialize};
//use uniswap_v3_sdk::{UniswapV3Position, Token, Pair};
use ethers::prelude::*;

#[derive(Debug, Deserialize)]
struct Config {
    uni_v3_pool_address: String,
    wallet_address: String,
    ethers_provider_url: String,
}

fn read_config(config_file_path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let mut file = File::open(config_file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get path to config file from command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <config-file-path>", args[0]);
        std::process::exit(1);
    }
    let config_file_path = &args[1];

    // Read configuration from file
    let config = read_config(config_file_path)?;

    // Set up ethers provider
    let provider = Provider::<Http>::try_from(config.ethers_provider_url)?;
    let block_number = provider.get_block_number().await?;
    println!("{block_number}");

    Ok(())
}
