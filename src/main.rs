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
    my_lp_position_id: usize,
}

#[derive(Debug, Clone)]
struct Position {
    token_0: Address,
    token_1: Address,
    fee: u32,
    tick_lower: i32,
    tick_upper: i32,
    liquidity: u128,
    tokens_owed_0: u128,
    tokens_owed_1: u128
}

fn read_config(config_file_path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let mut file = File::open(config_file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

async fn fetch_lp_position(
    provider: Provider<Http>,
    config: Config
) -> Result<Position, Box<dyn std::error::Error>> {
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
    let lp_position = uniswap_nfpm.positions(
        config.my_lp_position_id.into()
    ).call().await?;
    println!("LP Position: {:?}", lp_position);
    
    Ok(
        Position {
            token_0: lp_position.2,
            token_1: lp_position.3,
            fee: lp_position.4,
            tick_lower: lp_position.5,
            tick_upper: lp_position.6,
            liquidity: lp_position.7,
            tokens_owed_0: lp_position.10,
            tokens_owed_1: lp_position.11
        }
    )
}

async fn fetch_twap_tick(
    provider: Provider<Http>,
    config: Config,
    seconds: u32
) -> Result<(), Box<dyn std::error::Error>> {
    // create an instance of the UniswapV3Pool
    abigen!(
        UniswapV3Pool,
        "./src/abis/UniswapV3Pool.json",
    );
    let uni_v3_pool_address = config.uni_v3_pool_address.parse::<Address>()?;
    println!("got uni_v3_pool_address: {:?}", uni_v3_pool_address);
    let client = Arc::new(provider);
    let v3_pool = UniswapV3Pool::new(
        uni_v3_pool_address,
        client.clone()
    );
    println!("got v3_pool {:?}", v3_pool);

    // call the contract method to fetch price observations
    let input = vec!(
        seconds,
        u32::try_from(0).unwrap()
    );
    println!("input: {:?}", input);
    let observations = v3_pool.observe(input.into()).call().await?;
    println!("got observations {:?}", observations);

    // calculate the TWAP price as an arithmetic mean
    let tick_cumulatives = observations.0;
    let tick_cumulatives_delta = tick_cumulatives[1] - tick_cumulatives[0];
    println!("{:?}", tick_cumulatives_delta);
    let twap_tick = i32::try_from(tick_cumulatives_delta).unwrap() / i32::try_from(seconds).unwrap();
    println!("{:?}", twap_tick);

    Ok(())
}

async fn fetch_current_tick(
    provider: Provider<Http>,
    config: Config,
) -> Result<i32, Box<dyn std::error::Error>> {
    // create an instance of the UniswapV3Pool
    abigen!(
        UniswapV3Pool,
        "./src/abis/UniswapV3Pool.json",
    );
    let uni_v3_pool_address = config.uni_v3_pool_address.parse::<Address>()?;
    let client = Arc::new(provider);
    let v3_pool = UniswapV3Pool::new(
        uni_v3_pool_address,
        client.clone()
    );

    // call the contract method to fetch the current tick
    let slot_0 = v3_pool.slot_0().await?;
    println!("got slot0 {:?}", slot_0);
    let current_tick = slot_0.1;

    Ok(current_tick)
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

    loop {
        // get current LP position based on ID from config file
        let lp_position = fetch_lp_position(provider.clone(), config.clone()).await?;
        println!("{:?}", lp_position);

        // get current price as a tick
        let current_tick = fetch_current_tick(provider.clone(), config.clone()).await?;

        // check if current tick is outside position range
        // if above range, handle buy logic
        // if below range, handle sell logic

        // wait 5 minutes
        std::thread::sleep(Duration::from_secs(300));
    }

    Ok(())
}
