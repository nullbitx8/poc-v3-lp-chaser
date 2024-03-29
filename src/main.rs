use std::env;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use serde::{Deserialize};
use ethers::prelude::{abigen, Http, LocalWallet, Provider, SignerMiddleware};
use ethers::middleware::{Middleware, MiddlewareBuilder};
use ethers::types::{Address as EthersAddress, TransactionRequest, U256};
use alloy_primitives::{Address as AlloyAddress, Uint};
use uniswap_sdk_core::token;
use uniswap_sdk_core::prelude::{
    Address,
    Currency,
    CurrencyAmount,
    Token,
    Percent
};
use uniswap_v3_sdk::prelude::{
    CollectOptions,
    FeeAmount,
    get_collectable_token_amounts,
    get_position,
    get_pool,
    get_sqrt_ratio_at_tick,
    Pool,
    Position,
    nearest_usable_tick,
    remove_call_parameters,
    RemoveLiquidityOptions,
    sqrt_ratio_x96_to_price,
    u256_to_big_int,
};


#[derive(Debug, Deserialize, Clone)]
struct Config {
    my_lp_position_id: usize,
    range_percentage: f32,
    quote_token_size_in_usd: f32,
    uni_v3_pool_address: String,
    weth_address: String,
    usdc_address: String,
    weth_usdc_pool_address: String,
    uniswap_v3_factory_address: String,
    uniswap_nfpm_address: String,
    wallet_address: String,
    wallet_private_key: String,
    ethers_provider_url: String,
    chain_id: u64
}

fn read_config(config_file_path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let mut file = File::open(config_file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

/*
async fn get_provider(
    config: Config
) -> Result<Arc<Provider<Http>>, Box<dyn std::error::Error>> {
    let signer = config.wallet_private_key.as_str().parse::<LocalWallet>()?;

    let provider = Arc::new({
        Provider::<Http>::try_from(
            config.ethers_provider_url,
        )?.with_signer(signer)
    });

    Ok(provider)
}
*/

/*
async fn fetch_token(
    provider: Provider<Http>,
    address: String,
    chain_id: u64
) -> Result<Token, Box<dyn std::error::Error>> {
    let client = Arc::new(provider);

    let h160_address = address.clone().parse::<EthersAddress>()?;
    abigen!(
        IERC20,
        "./src/abis/IERC20.json",
    );
    let token_contract = IERC20::new(
        h160_address,
        client
    );

    let decimals = token_contract.decimals().call().await?;
    let symbol = token_contract.symbol().call().await?;
    let name = token_contract.name().call().await?;

    let token = token!(
        chain_id,
        address.clone(),
        decimals,
        symbol,
        name
    );

    Ok(token)
}
*/

/*
async fn fetch_pool(
    provider: Provider<Http>,
    address: String,
    chain_id: u32
) -> Result<Pool, Box<dyn std::error::Error>> {
    let client = Arc::new(provider.clone());
    let address = address.parse::<Address>()?;

    abigen!(
        UniswapV3Pool,
        "./src/abis/UniswapV3Pool.json",
    );
    let v3_pool = UniswapV3Pool::new(
        address.clone(),
        client
    );

    let token0 = v3_pool.token_0().call().await?;
    let token0 = fetch_token(
        provider.clone(),
        token0.clone().to_string(),
        chain_id
    ).await?;

    let token1 = v3_pool.token_1().call().await?;
    let token1 = fetch_token(
        provider.clone(),
        token1.clone().to_string(),
        chain_id
    ).await?;

    let slot0 = v3_pool.slot_0().call().await?;
    let fee_amount = v3_pool.fee().call().await?;
    let liquidity = v3_pool.liquidity().call().await?;

    Pool::new(
        token0,
        token1,
        fee_amount.into(),
        slot0.0,  // sqrt ratio
        liquidity,
    )
}
*/

/*
async fn fetch_lp_position(
    provider: Provider<Http>,
    config: Config
) -> Result<Position, Box<dyn std::error::Error>> {
    // fetch the pool
    let pool = fetch_pool(
        provider.clone(),
        config.uni_v3_pool_address,
        config.chain_id
    );

    // fetch our position
    let client = Arc::new(provider);

    abigen!(
        NFPM,
        "./src/abis/NonfungiblePositionManager.json",
    );
    let nfpm_address = config.uniswap_nfpm_address.parse::<Address>()?;
    let uniswap_nfpm = NFPM::new(
        nfpm_address,
        client.clone()
    );

    // call the contract method to fetch the LP position
    let lp_position = uniswap_nfpm.positions(
        config.my_lp_position_id.into()
    ).call().await?;

    Position::new(
        pool.clone(),
        lp_position.7,  // liquidity
        lp_position.5,  // tick lower
        lp_position.6   // tick upper
    )
}
*/

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
    let uni_v3_pool_address = config.uni_v3_pool_address.parse::<EthersAddress>()?;
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
    let uni_v3_pool_address = config.uni_v3_pool_address.parse::<EthersAddress>()?;
    let client = Arc::new(provider);
    let v3_pool = UniswapV3Pool::new(
        uni_v3_pool_address,
        client.clone()
    );

    // call the contract method to fetch the current tick
    let slot_0 = v3_pool.slot_0().await?;
    let current_tick = slot_0.1;

    Ok(current_tick)
}

async fn get_usdc_price_of_weth(
    provider: Provider<Http>,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    // get sqrtPriceX96 from pool
    let pool = get_pool(
        config.chain_id,
        config.uniswap_v3_factory_address.parse::<AlloyAddress>()?,
        config.weth_address.parse::<AlloyAddress>()?,
        config.usdc_address.parse::<AlloyAddress>()?,
        FeeAmount::MEDIUM,
        Arc::new(provider.clone()),
        None
    ).await?;
    let sqrt_price = pool.sqrt_ratio_x96;

    // convert to price
    let price = sqrt_ratio_x96_to_price(
        sqrt_price,
        pool.token0,
        pool.token1
    );
    println!("price: {:?}", price);

    // return amount including decimals
    Ok(())
}

async fn get_size_in_weth(
    amount_in_usdc: f32
) -> Result<f32, Box<dyn std::error::Error>> {
    // get current price of WETH based on pool
    // TODO
    
    Ok(69.420)
}

fn calc_new_ticks(current_tick: i32, percentage: f32) -> (i32, i32) {
    // calculate the upper and lower ticks by bounding them to a
    // price change percentage
    //
    // 1 tick = 0.01% in price change
    //
    let tick_change = (percentage / 0.01 as f32).round() as i32;
    let upper_tick = current_tick + tick_change;
    let lower_tick = current_tick - tick_change;

    (lower_tick, upper_tick)
}

fn convert_to_ethers_u256(alloy_u256: Uint<256, 4>) -> U256 {
    let inner_value: [u8; 32] = alloy_u256.to_le_bytes(); // Convert to bytes in little-endian format
    let mut bytes = [0; 32];
    bytes.copy_from_slice(&inner_value); // Copy the bytes into a fixed-size array

    // Create an ethers U256 from the bytes
    U256::from(bytes)
}

async fn remove_liquidity_collect_fees(
    provider: Provider<Http>,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("in remove_liquidity");

    // get position using token id
    let position = get_position(
        config.chain_id,
        config.uniswap_nfpm_address.parse::<AlloyAddress>()?,
        config.my_lp_position_id.to_string().parse().unwrap(),
        Arc::new(provider.clone()),
        None
    ).await?;

    // get collectible fees amounts 
    let collectible_token_amounts = get_collectable_token_amounts(
        config.chain_id,
        config.uniswap_nfpm_address.parse::<AlloyAddress>()?,
        config.my_lp_position_id.to_string().parse().unwrap(),
        Arc::new(provider.clone()),
        None
    ).await?;
    let currency_owed0_amount = CurrencyAmount::from_raw_amount(
        Currency::Token(position.pool.token0.clone()),
        u256_to_big_int(collectible_token_amounts.0),
    )?;
    let currency_owed1_amount = CurrencyAmount::from_raw_amount(
        Currency::Token(position.pool.token1.clone()),
        u256_to_big_int(collectible_token_amounts.1),
    )?;

    // set deadline 2 minutes from now
    let deadline = (
        SystemTime::now() + Duration::from_secs(2 * 60)
    ).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

    // configure remove liquidity options
    let options = RemoveLiquidityOptions {
        token_id: config.my_lp_position_id.to_string().parse().unwrap(),
        liquidity_percentage: Percent::new(100,100),
        slippage_tolerance: Percent::new(1, 100),
        deadline: deadline.to_string().parse().unwrap(),
        burn_token: false,
        permit: None,
        collect_options: CollectOptions {
            token_id: config.my_lp_position_id.to_string().parse().unwrap(),
            expected_currency_owed0: currency_owed0_amount,
            expected_currency_owed1: currency_owed1_amount,
            recipient: config.wallet_address.parse().unwrap()
        }
    };

    // prepare the input for nfpm remove liquidity method
    let method_params = remove_call_parameters(
        &position,
        options
    )?;
    let value = convert_to_ethers_u256(method_params.value);

    // prepare tx
    let signer = config.wallet_private_key.as_str().parse::<LocalWallet>()?;
    let client = provider.with_signer(signer);
    let tx = TransactionRequest::new()
        .to(config.uniswap_nfpm_address)
        .value(value)
        .data(method_params.calldata);

    // send tx to mempool
    let pending_tx = client.send_transaction(tx, None).await?;

    // get the mined tx
    let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
    let tx = client.get_transaction(receipt.transaction_hash).await?;

    println!("Sent tx: {}\n", serde_json::to_string(&tx)?);
    println!("Tx receipt: {}", serde_json::to_string(&receipt)?);


    Ok(())
}

async fn adjust_lower<P>(
    provider: Provider<Http>,
    config: Config,
    lp_position: Position<P>,
    new_ticks: &(i32, i32)
) -> Result<(), Box<dyn std::error::Error>> {
    // undo the current LP position and collect fees
    remove_liquidity_collect_fees(provider.clone(), config.clone()).await?;

    // calculate amounts needed for new position
    // get nearest usable ticks
    let lower_tick = nearest_usable_tick(new_ticks.0, lp_position.pool.tick_spacing());
    let upper_tick = nearest_usable_tick(new_ticks.1, lp_position.pool.tick_spacing());
    
    // get sqrt ratio at the ticks
    let sqrt_ratio_lower = get_sqrt_ratio_at_tick(lower_tick);
    let sqrt_ratio_upper = get_sqrt_ratio_at_tick(upper_tick);

    // get amount of token0 to use
    let token0_amount = 
        if lp_position.pool.token0.address == config.weth_address.clone().parse::<AlloyAddress>()? || 
            lp_position.pool.token1.address == config.weth_address.clone().parse::<AlloyAddress>()? {
            get_size_in_weth(config.quote_token_size_in_usd.clone()).await?
    } else if lp_position.pool.token0.address == config.usdc_address.clone().parse::<AlloyAddress>()? ||
        lp_position.pool.token1.address == config.usdc_address.clone().parse::<AlloyAddress>()? {
            config.quote_token_size_in_usd
        }
      else { panic!("Neither token0 nor token1 are weth or usdc"); };


    // get liquidity using lower and upper ticks + amount of token0 to use
    // use liquidity + lower and upper ticks to get amount of token1 needed 

    // sell tokens as needed

    // create new position

    // save new position ID to config file

    Ok(())
}

async fn adjust_higher<P>(
    provider: Provider<Http>,
    config: Config,
    lp_position: Position<P>,
    new_ticks: &(i32, i32)
) -> Result<(), Box<dyn std::error::Error>> {
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
    let provider = Provider::<Http>::try_from(
        config.ethers_provider_url.clone(),
    )?;
    let block_number = provider.get_block_number().await?;
    println!("{block_number}");

    loop {
        // get current LP position based on ID from config file
        //let lp_position = fetch_lp_position(provider.clone(), config.clone()).await?;
        let lp_position = get_position(
            config.chain_id.clone(),
            config.uniswap_nfpm_address.clone().parse::<AlloyAddress>()?,
            config.my_lp_position_id.clone().to_string().parse().unwrap(),
            Arc::new(provider.clone()),
            None
        ).await?;
        println!("lp position: {:?}", lp_position);

        // get current price as a tick
        let current_tick = fetch_current_tick(provider.clone(), config.clone()).await?;
        println!("current tick: {:?}", current_tick);

        // calculate new upper and lower ticks
        let new_ticks = calc_new_ticks(current_tick, config.range_percentage);

        get_usdc_price_of_weth(provider.clone(), config.clone()).await?;

        // check if current tick is outside of our position range
        // and adjust the position as necessary
        if current_tick < lp_position.tick_lower {
            adjust_lower(
                provider.clone(),
                config.clone(),
                lp_position.clone(),
                &new_ticks
            );
        }
        if current_tick > lp_position.tick_upper {
            adjust_higher(
                provider.clone(),
                config.clone(),
                lp_position.clone(),
                &new_ticks
            );
        }

        // wait 5 minutes
        std::thread::sleep(Duration::from_secs(300));
    }

    Ok(())
}
