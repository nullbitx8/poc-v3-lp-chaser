use std::env;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize};
use ethers::prelude::*;


#[derive(Debug, Deserialize, Clone)]
struct Config {
    uni_v3_pool_address: String,
    wallet_address: String,
    ethers_provider_url: String,
    uniswap_nfpm_address: String,
}

fn read_config(config_file_path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let mut file = File::open(config_file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

async fn fetch_lp_position(provider: Provider<Http>, config: Config) -> Result<(), Box<dyn std::error::Error>> {
    abigen!(
        NFPM,
        "./src/abis/NonfungiblePositionManager.json",
    );
    let nfpm_address = config.uniswap_nfpm_address.parse::<Address>()?;
    let client = Arc::new(provider);
    let uniswap_nfpm = NFPM::new(
        nfpm_address,
        client.clone()
    );

    // call the contract method to fetch the LP position
    let lp_position = uniswap_nfpm.positions(3628.into()).call().await?;
    println!("LP Position: {:?}", lp_position);
    
    Ok(())
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
    let provider = Provider::<Http>::try_from(config.ethers_provider_url.clone())?;
    let block_number = provider.get_block_number().await?;
    println!("{block_number}");

    // get current LP position based on ID from config file
    let lp_position = fetch_lp_position(provider.clone(), config.clone()).await?;

    //println!("{lp_position}");

    Ok(())
}
