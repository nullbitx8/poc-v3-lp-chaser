// TODO
//  - [ ] refactor sending tx into its own function that accepts signer, value, calldata
//  - [ ] Make Config global
//  - [ ] Make Provider global, and a signer
//  - [ ] Add logging
//  - [ ] Add tests
use std::env;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};
use ethers::utils::keccak256;
use ethers::prelude::{
    abigen,
    Address as EthersAddress,
    BlockId,
    BlockNumber,
    H160,
    H256,
    Http,
    LocalWallet,
    Log,
    Middleware,
    MiddlewareBuilder,
    Provider,
    Signer,
    SignerMiddleware,
    TransactionRequest,
    U256,
};
use alloy_primitives::{Address as AlloyAddress, Uint};
use num_traits::ToPrimitive;
use uniswap_sdk_core::token;
use uniswap_sdk_core::prelude::{
    Address,
    Currency,
    CurrencyAmount,
    CurrencyLike,
    Percent,
    Price,
    Rounding,
    Token,
    TradeType,
};
use uniswap_v3_sdk::prelude::{
    add_call_parameters,
    AddLiquidityOptions,
    AddLiquiditySpecificOptions,
    CollectOptions,
    EphemeralTickDataProvider,
    FeeAmount,
    get_amount_1_delta,
    get_collectable_token_amounts,
    get_position,
    get_pool,
    get_sqrt_ratio_at_tick,
    max_liquidity_for_amount0_imprecise,
    MintSpecificOptions,
    nearest_usable_tick,
    NoTickDataProvider,
    Pool,
    Position,
    remove_call_parameters,
    RemoveLiquidityOptions,
    Route,
    sqrt_ratio_x96_to_price,
    swap_call_parameters,
    SwapOptions,
    ToEthers,
    Trade,
    u256_to_big_int,
};


#[derive(Clone, Debug, Deserialize, Serialize)]
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

async fn fetch_pool(
    provider: Provider<Http>,
    config: Config,
) -> Result<Pool<NoTickDataProvider>, Box<dyn std::error::Error>> {
    let client = Arc::new(provider.clone());

    abigen!(
        UniswapV3Pool,
        "./src/abis/UniswapV3Pool.json",
    );
    let v3_pool = UniswapV3Pool::new(
        config.uni_v3_pool_address.parse::<EthersAddress>()?,
        client
    );

    let token0 = v3_pool.token_0().call().await?;
    let token1 = v3_pool.token_1().call().await?;
    let fee_amount = v3_pool.fee().call().await?;
    let fee_amount = match fee_amount {
        100 => FeeAmount::LOWEST,
        500 => FeeAmount::LOW,
        3000 => FeeAmount::MEDIUM,
        10000 => FeeAmount::HIGH,
        _ => panic!("invalid fee amount")
    };

    // get pool
    let pool = get_pool(
        config.chain_id,
        config.uniswap_v3_factory_address.parse::<AlloyAddress>()?,
        token0.to_fixed_bytes().into(),
        token1.to_fixed_bytes().into(),
        fee_amount,
        Arc::new(provider),
        None
    ).await?;

    Ok(pool)
}

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

/*
 * Extract token ID from logs.
 * It is the first parameter of the IncreaseLiquidity event.
 */
fn extract_token_id_from_logs(
    logs: Vec<Log>
) -> Result<U256, Box<dyn std::error::Error>> {
    const event_sig: &str = "IncreaseLiquidity(uint256,uint128,uint256,uint256)";
    let event_sig_hashed = H256::from(keccak256(event_sig.as_bytes()));

    for log in logs {
        let topics: Vec<H256> = log.topics;
        let data: Vec<u8> = log.data.to_vec();

        // decode IncreaseLiquidity event
        if topics.len() > 0 && topics[0] == event_sig_hashed {
            let token_id = U256::from_big_endian(&topics[1].as_bytes()[29..32]);
            return Ok(token_id);
        }
    }
    panic!("IncreaseLiquidity event or not found in logs");
}

async fn erc20_balance_of(
    provider: Provider<Http>,
    token_address: String,
    target_address: String
) -> Result<String, Box<dyn std::error::Error>> {
    // create an instance of the IERC20 contract
    abigen!(
        IERC20,
        "./src/abis/IERC20.json",
    );
    let token_address = token_address.parse::<EthersAddress>()?;
    let client = Arc::new(provider);
    let token = IERC20::new(
        token_address,
        client.clone()
    );

    // call the contract method to fetch the balance of the wallet
    let balance = token.balance_of(
        target_address.parse::<EthersAddress>()?
    ).call().await?;

    Ok(balance.to_string())
}

async fn assert_balance(
    provider: Provider<Http>,
    config: Config,
    token: Token,
    amount: U256
) -> Result<(), Box<dyn std::error::Error>> {
    let balance = erc20_balance_of(
        provider.clone(),
        token.address.to_string(),
        config.wallet_address.clone()
    ).await?;

    if balance.parse::<U256>().unwrap() < amount {
        panic!(
            "insufficient balance of token {:?}; balance: {:?}, needed: {:?}",
            token.symbol.unwrap(),
            balance,
            amount
        );
    }

    Ok(())
}

async fn erc20_approve(
    provider: Provider<Http>,
    config: Config,
    token: Token,
    spender: String,
    amount: U256
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(provider.clone());
    let caller = config.wallet_address.parse::<EthersAddress>()?;
    let spender = spender.parse::<EthersAddress>()?;

    // create an instance of the IERC20 contract
    abigen!(
        IERC20,
        "./src/abis/IERC20.json",
    );
    let token_address = token.address.to_string().parse::<EthersAddress>()?;
    let token = IERC20::new(
        token_address.clone(),
        client
    );

    // check if spender is already approved to spend the token
    let allowance = token.allowance(
        caller.clone(),
        spender.clone()
    ).call().await?;
    if allowance >= amount {
        return Ok(());
    }

    // call the contract method to approve the spender to spend the token
    println!("approving {spender} to spend {token_address} MAX U256");
    let function_call = token.approve(
        spender.clone(),
        U256::MAX
    );
    let calldata = function_call.calldata().unwrap();

    let signer = config.wallet_private_key.as_str().parse::<LocalWallet>()?;            
    let signer = signer.with_chain_id(config.chain_id);
    let client = provider.with_signer(signer);                                          
    let tx = TransactionRequest::new()                                                  
        .to(token_address)                                                              
        .data(calldata);                                                                
    let pending_tx = client.send_transaction(tx, None).await?;
    
    // get the mined tx
    let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
    let tx = client.get_transaction(receipt.transaction_hash).await?;

    println!("Sent tx: {}\n", serde_json::to_string(&tx)?);
    println!("Tx receipt: {}", serde_json::to_string(&receipt)?);

    Ok(())
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
) -> Result<String, Box<dyn std::error::Error>> {
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

    // convert to price
    let price = sqrt_ratio_x96_to_price(
        pool.sqrt_ratio_x96,
        pool.token0,
        pool.token1.clone()
    ).unwrap();
    let fixed = price.to_fixed(4, Rounding::RoundHalfUp);

    Ok(fixed)
}

async fn get_size_in_weth(
    provider: Provider<Http>,
    config: Config,
) -> Result<String, Box<dyn std::error::Error>> {
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

    // convert to price
    let price = sqrt_ratio_x96_to_price(
        pool.sqrt_ratio_x96,
        pool.token0,
        pool.token1.clone()
    );
    let price = price.unwrap();
    println!(
        "price of WETH in USD: {:?}",
        price.to_significant(pool.token1.decimals, Rounding::RoundHalfUp).unwrap()
    );

    // calculate amount of WETH needed based on config.quote_token_size_in_usd
    let inverted = price.clone().invert();
    let quote = inverted.quote(
        CurrencyAmount::from_raw_amount(
            price.quote_currency.clone(),
            config.quote_token_size_in_usd.clone() as u32 * 10u32.pow(6)
        ).unwrap()
    ).unwrap();

    // return amount including decimals
    Ok(quote.to_exact())
}

async fn swap_token_for_token(
    provider: Provider<Http>,
    config: Config,
    token_in: Token,
    token_out: Token,
    pool_fee: FeeAmount,
    amount: String,
    tick_provider: EphemeralTickDataProvider
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = get_pool(
        config.chain_id,
        config.uniswap_v3_factory_address.parse::<AlloyAddress>()?,
        token_in.address.clone(),
        token_out.address.clone(),
        pool_fee,
        Arc::new(provider.clone()),
        None
    ).await?;

    println!("getting pool with tick data provider: {:?}", pool);
    let pool = Pool::new_with_tick_data_provider(
        pool.token0.clone(),
        pool.token1.clone(),
        pool.fee.clone(),
        pool.sqrt_ratio_x96.clone(),
        pool.liquidity.clone(),
        tick_provider
    )?;
    println!("got pool with tick data provider: {:?}", pool);

    // get the price
    let price = sqrt_ratio_x96_to_price(
        pool.sqrt_ratio_x96,
        token_in.clone(),
        token_out.clone()
    ).unwrap();

    // get the amount of token_out needed
    let amount = CurrencyAmount::from_raw_amount(
        token_in.clone(),
        u256_to_big_int(amount.parse::<Uint<256, 4>>().unwrap())
    ).unwrap();
    let amount_out = price
        .quote(amount.clone())?
        .to_exact();

    // set deadline 2 minutes from now
    let deadline = (
        SystemTime::now() + Duration::from_secs(2 * 60)
    ).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

    // configure swap options
    let options = SwapOptions {
        slippage_tolerance: Percent::new(1, 100),
        recipient: config.wallet_address.parse().unwrap(),
        deadline: deadline.to_string().parse().unwrap(),
        input_token_permit: None,
        sqrt_price_limit_x96: None,
        fee: None,
    };

    let trade = Trade::from_route(
        Route::new(vec![pool.clone()], token_in.clone(), token_out.clone()),
        amount.clone(),
        TradeType::ExactInput,
    )?;

    // prepare the input for router swap
    let method_params = swap_call_parameters(&mut [trade], options.clone()).unwrap();
    let value = convert_to_ethers_u256(method_params.value);

    // prepare tx
    let signer = config.wallet_private_key.as_str().parse::<LocalWallet>()?;
    let client = provider.with_signer(signer);
    let tx = TransactionRequest::new()
        .to(config.uniswap_nfpm_address)
        .value(value)
        .data(method_params.calldata);
    let pending_tx = client.send_transaction(tx, None).await?;
    
    // get the mined tx
    let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
    let tx = client.get_transaction(receipt.transaction_hash).await?;

    println!("Sent tx: {}\n", serde_json::to_string(&tx)?);
    println!("Tx receipt: {}", serde_json::to_string(&receipt)?);

    Ok(())
}

// function to take a string decimal and remove any decimal points or leading zeros
fn remove_decimals(decimal: String) -> String {
    let decimal = decimal.replace(".", "");
    let decimal = decimal.trim_start_matches("0").to_string();
    decimal
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

async fn create_lp_position(
    provider: Provider<Http>,
    config: Config,
    config_file_path: &str
) -> Result<(), Box<dyn std::error::Error>> {
    println!("creating liquidity position");

    // query v3 pool using address from config file
    let pool = fetch_pool(
        provider.clone(),
        config.clone()
    ).await?;
    println!("got pool: {:?}", pool);

    // get current price as a tick
    println!("current tick: {:?}", pool.tick_current);

    // calculate new upper and lower ticks
    let new_ticks = calc_new_ticks(pool.tick_current, config.range_percentage);

    // calculate amounts needed for new position
    let lower_tick = nearest_usable_tick(new_ticks.0, pool.tick_spacing());
    let upper_tick = nearest_usable_tick(new_ticks.1, pool.tick_spacing());

    // get sqrt ratio at the ticks
    let sqrt_ratio_lower = get_sqrt_ratio_at_tick(lower_tick).unwrap();
    let sqrt_ratio_upper = get_sqrt_ratio_at_tick(upper_tick).unwrap();

    // get token0 amount
    let token0_amount = 
        if pool.token0.address == config.weth_address.parse::<AlloyAddress>()? {
            get_size_in_weth(provider.clone(), config.clone()).await?
        }
        else if pool.token0.address == config.usdc_address.parse::<AlloyAddress>()? {
            (config.quote_token_size_in_usd as u32 * 10u32.pow(6)).to_string()
        }
        else { panic!("Neither token0 nor token1 are weth or usdc"); };

    // remove decicmals or leading zeros
    let token0_amount = remove_decimals(token0_amount);
    println!("token0 amount needed as a whole number: {:?}", token0_amount);

    // get liquidity using lower and upper ticks + amount of token0 to use
    // get token0_amount as a Uint from ruint library
    let token0_amount = Uint::<256, 4>::from_str_radix(token0_amount.as_str(), 10).unwrap();

    let liquidity_for_token_0 = max_liquidity_for_amount0_imprecise(
        pool.sqrt_ratio_x96.clone(),
        sqrt_ratio_upper.clone(),
        token0_amount
    );
    println!("liquidity for token 0: {:?}", liquidity_for_token_0);

    // use liquidity + lower and upper ticks to get amount of token1 needed 
    let liquidity_for_token_0 = liquidity_for_token_0.to_u128().unwrap();

    let token1_amount = get_amount_1_delta(
        sqrt_ratio_lower.clone(),
        pool.sqrt_ratio_x96.clone(),
        liquidity_for_token_0,
        false
    ).unwrap();
    println!("token1 amount needed as a whole number: {:?}", token1_amount.to_string());

    // configure options for adding liquidity to the pool
    let deadline = (
        SystemTime::now() + Duration::from_secs(2 * 60)
    ).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

    // configure add liquidity options
    let options = AddLiquidityOptions {
        slippage_tolerance: Percent::new(1, 100),
        deadline: deadline.to_string().parse().unwrap(),
        use_native: None,
        token0_permit: None,
        token1_permit: None,
        specific_opts: AddLiquiditySpecificOptions::Mint(
            MintSpecificOptions {
                recipient: config.wallet_address.parse().unwrap(),
                create_pool: false,
            }
        )
    };

    // create desired position
    let position = Position::new(
        pool.clone(),
        liquidity_for_token_0,
        lower_tick,
        upper_tick
    );

    // prepare the input for nfpm add liquidity method
    let method_params = add_call_parameters(
        &mut position.clone(),
        options
    ).unwrap();

    let token0_amount = token0_amount.to_ethers();
    let mut token1_amount = token1_amount.to_ethers();
    println!("token0 amount: {:?}", token0_amount);
    println!("token1 amount: {:?}", token1_amount);

    // TODO figure out why nfpm calculates token1 needed as 1 instead of 0
    // and if we are doing the calculation wrong, then fix this
    if token1_amount == U256::zero() {
        token1_amount = U256::from(1);
    }
    println!("token1 amount: {:?}", token1_amount);

    // assert sufficient balance of tokens
    assert_balance(
        provider.clone(),
        config.clone(),
        pool.token0.clone(),
        token0_amount.clone()
    ).await?;

    assert_balance(
        provider.clone(),
        config.clone(),
        pool.token1.clone(),
        token1_amount.clone()
    ).await?;

    // allow nfpm to spend token0 and token1
    erc20_approve(
        provider.clone(),
        config.clone(),
        pool.token0.clone(),
        config.uniswap_nfpm_address.clone(),
        token0_amount
    ).await?;
    erc20_approve(
        provider.clone(),
        config.clone(),
        pool.token1.clone(),
        config.uniswap_nfpm_address.clone(),
        token1_amount
    ).await?;

    // prepare tx
    let value = convert_to_ethers_u256(method_params.value);
    let signer = config.wallet_private_key.as_str().parse::<LocalWallet>()?;
    let signer = signer.with_chain_id(config.chain_id);
    let client = provider.with_signer(signer);
    let tx = TransactionRequest::new()
        .to(config.uniswap_nfpm_address.parse::<EthersAddress>()?)
        .value(value)
        .data(method_params.calldata);

    // send tx to mempool
    println!("sending liquidity add tx to mempool");
    let pending_tx = client.send_transaction(tx, None).await?;

    // get the mined tx
    let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
    println!("Tx mined, hash: {:?}", receipt.transaction_hash);

    // get token ID from logs in tx receipt
    let token_id = extract_token_id_from_logs(receipt.logs)?;
    println!("New LP token ID: {:?}", token_id);

    // save token ID to config file
    let mut config = config.clone();
    config.my_lp_position_id = token_id.as_usize();
    let config = serde_json::to_string_pretty(&config)?;
    std::fs::write(config_file_path, config)?;
    println!("Wrote new LP position token ID to config file");

    Ok(())
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
    //remove_liquidity_collect_fees(provider.clone(), config.clone()).await?;

    // calculate amounts needed for new position
    let lower_tick = nearest_usable_tick(new_ticks.0, lp_position.pool.tick_spacing());
    let upper_tick = nearest_usable_tick(new_ticks.1, lp_position.pool.tick_spacing());
    
    // get sqrt ratio at the ticks
    let sqrt_ratio_lower = get_sqrt_ratio_at_tick(lower_tick.clone()).unwrap();
    let sqrt_ratio_upper = get_sqrt_ratio_at_tick(upper_tick.clone()).unwrap();

    // panic if both tokens are quote tokens
    if lp_position.pool.token0.address == config.weth_address.clone().parse::<AlloyAddress>()?
        && lp_position.pool.token1.address == config.usdc_address.clone().parse::<AlloyAddress>()? {
            panic!("token0 is weth and token1 is usdc");
    }
    if lp_position.pool.token0.address == config.usdc_address.clone().parse::<AlloyAddress>()?
        && lp_position.pool.token1.address == config.weth_address.clone().parse::<AlloyAddress>()? {
            panic!("token0 is usdc and token1 is weth");
    }

    // get token0 amount
    let token0_amount = 
        if lp_position.pool.token0.address == config.weth_address.clone().parse::<AlloyAddress>()? || 
            lp_position.pool.token1.address == config.weth_address.clone().parse::<AlloyAddress>()? {
                println!("getting amount of tokens to provide in weth");
                get_size_in_weth(provider.clone(), config.clone()).await?
        }
        else if lp_position.pool.token0.address == config.usdc_address.clone().parse::<AlloyAddress>()? ||
            lp_position.pool.token1.address == config.usdc_address.clone().parse::<AlloyAddress>()? {
                println!("getting amount of tokens to provide in usdc");
                (config.quote_token_size_in_usd as u32 * 10u32.pow(6)).to_string()
        }
        else { panic!("Neither token0 nor token1 are weth or usdc"); };

    println!("token0 amount: {:?}", token0_amount);

    // remove decicmals or leading zeros
    let token0_amount = remove_decimals(token0_amount);
    println!("token0 amount needed as a whole number: {:?}", token0_amount);

    // get liquidity using lower and upper ticks + amount of token0 to use
    // get token0_amount as a Uint from ruint library
    let token0_amount = Uint::<256, 4>::from_str_radix(token0_amount.as_str(), 10).unwrap();

    let liquidity_for_token_0 = max_liquidity_for_amount0_imprecise(
        sqrt_ratio_lower.clone(),
        sqrt_ratio_upper.clone(),
        token0_amount
    );
    println!("liquidity for token 0: {:?}", liquidity_for_token_0);

    // use liquidity + lower and upper ticks to get amount of token1 needed 
    let liquidity_for_token_0 = liquidity_for_token_0.to_u128().unwrap();

    let token1_amount = get_amount_1_delta(
        sqrt_ratio_lower.clone(),
        sqrt_ratio_upper.clone(),
        liquidity_for_token_0,
        false
    ).unwrap();
    let token1_amount = token1_amount.to_string();
    println!("token1 amount needed as a whole number: {:?}", token1_amount);

    // get wallet_address balance of token1
    let wallet_balance_token1 = erc20_balance_of(
        provider.clone(),
        lp_position.pool.token1.address.to_string(),
        config.wallet_address.clone()
    ).await?;
    println!("wallet balance of token1: {:?}", wallet_balance_token1);

    // get wallet_address balance of token1
    let wallet_balance_token0 = erc20_balance_of(
        provider.clone(),
        lp_position.pool.token0.clone().address.to_string(),
        config.wallet_address.clone()
    ).await?;
    println!("wallet balance of token0: {:?}", wallet_balance_token0);

    // calc the difference of token1 balance and token1_amount
    let token1_difference = wallet_balance_token1.parse::<u128>().unwrap() - token1_amount.parse::<u128>().unwrap();
    println!("token1 difference: {:?}", token1_difference);

    // create ephemeral tick data provider
    println!("creating tick provider");
    let tick_provider = EphemeralTickDataProvider::new(
        config.uni_v3_pool_address.clone().parse::<AlloyAddress>()?,
        Arc::new(provider.clone()),
        Some(lower_tick.clone()),
        Some(upper_tick.clone()),
        None
    ).await?;
    println!("created tick provider: {:?}", tick_provider);

    // sell the difference of token1 for token0 on uniswap
    swap_token_for_token(
        provider.clone(),
        config.clone(),
        lp_position.pool.token1.clone(),
        lp_position.pool.token0.clone(),
        lp_position.pool.fee.clone(),
        token1_difference.to_string(),
        tick_provider
    ).await?;

    // then provide token0 and token1 to the pool
    // then mint the new LP position
    // then save the new LP position ID to the config file
    // then return

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
    println!("block: {block_number}");

    // if my_lp_position_id is 0, then create a new LP position
    // and save the ID to the config file
    if config.my_lp_position_id == 0 {
        create_lp_position(
            provider.clone(),
            config.clone(),
            &args[1]
        ).await?;
    }

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
        let current_tick = lp_position.pool.tick_current.clone();
        println!("current tick: {:?}", current_tick);

        // calculate new upper and lower ticks
        let new_ticks = calc_new_ticks(current_tick, config.range_percentage);

        // check if current tick is outside of our position range
        // and adjust the position as necessary
        adjust_lower(
            provider.clone(),
            config.clone(),
            lp_position.clone(),
            &new_ticks
        ).await?;
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
