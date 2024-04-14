// TODO
//  - [ ] refactor sending tx into its own function that accepts signer, value, calldata
//  - [ ] replace convert_to_ethers_u256 with to_ethers()
//  - [ ] Make Config global
//  - [ ] Make Provider global, and a signer
//  - [ ] Add logging
//  - [ ] Add tests
use std::env;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};
use ethers::utils::{
    format_units,
    keccak256,
    parse_units,
};
use ethers::prelude::{
    abigen,
    Address as EthersAddress,
    H256,
    Http,
    LocalWallet,
    Log,
    Middleware,
    MiddlewareBuilder,
    Provider,
    Signer,
    TransactionReceipt,
    TransactionRequest,
    U64,
    U256,
};
use alloy_primitives::{Address as AlloyAddress, Uint};
use num_traits::ToPrimitive;
use uniswap_sdk_core::prelude::{
    Currency,
    CurrencyAmount,
    MAX_UINT256,
    Percent,
    Rounding,
    Token,
    TradeType,
};
use uniswap_v3_math::tick_math::{MAX_TICK, MIN_TICK};
use uniswap_v3_sdk::prelude::{
    add_call_parameters,
    AddLiquidityOptions,
    AddLiquiditySpecificOptions,
    CollectOptions,
    EphemeralTickDataProvider,
    FeeAmount,
    get_collectable_token_amounts,
    get_position,
    get_pool,
    get_sqrt_ratio_at_tick,
    max_liquidity_for_amount0_imprecise,
    max_liquidity_for_amount1,
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
    ToAlloy,
    ToEthers,
    Trade,
    u256_to_big_int,
};


#[derive(Clone, Debug, Deserialize, Serialize)]
struct Config {
    my_lp_position_id: usize,
    range_percentage: f32,
    quote_token_size_in_usd: f32,
    seconds_to_wait: u64,
    add_liq_slippage_pct: f32,
    swap_slippage_pct: f32,
    uni_v3_pool_address: String,
    weth_address: String,
    usdc_address: String,
    weth_usdc_pool_address: String,
    uniswap_v3_factory_address: String,
    uniswap_nfpm_address: String,
    uniswap_router_address: String,
    tick_data_provider_range: f32,
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

fn write_config(
    config: Config,
    config_file_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("");
    println!("Writing config to file at {:?}", config_file_path);

    let config = serde_json::to_string_pretty(&config)?;
    std::fs::write(config_file_path, config)?;

    println!("Wrote new LP position token ID to config file");
    println!("");

    Ok(())
}

async fn send_tx_to_mempool(
    provider: Provider<Http>,
    config: Config,
    tx: TransactionRequest
) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
    // configure signer
    let signer = config.wallet_private_key.as_str().parse::<LocalWallet>()?;
    let signer = signer.with_chain_id(config.chain_id);
    let client = provider.with_signer(signer);

    // send tx to mempool
    println!("sending to mempool: {:?}", tx);
    let pending_tx = client.send_transaction(tx.clone(), None).await?;

    // get the mined tx
    let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
    println!("Tx mined, hash: {:?}", receipt.transaction_hash);
    println!("Receipt status: {:?}", receipt.status);

    // if tx failed, retry up to 2 times
    // increasing gas limit by 10% and gas price by 10% each time
    if receipt.status == Some(U64::from(0)) {
        println!("Tx failed, retrying up to 2 times");
        println!("Raising gas limit and gas price by 10% each time");
        println!("");
        let mut tx = tx.clone();
        let mut gas_price = receipt.effective_gas_price.unwrap();
        let mut gas_limit = receipt.gas_used.unwrap();
        gas_price = gas_price + gas_price / 10;
        gas_limit = gas_limit + gas_limit / 10;
        tx = tx.gas(gas_limit).gas_price(gas_price);

        println!("sending to mempool: {:?}", tx);
        let pending_tx = client.send_transaction(tx.clone(), None).await?;
        let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
        println!("Tx mined, hash: {:?}", receipt.transaction_hash);
        println!("Receipt status: {:?}", receipt.status);

        if receipt.status == Some(U64::from(1)) {
            return Ok(receipt);
        } else {
            println!("Tx failed, retrying 1 more time");
            println!("Raising gas limit and gas price by 10% each");
            println!("");
            let mut tx = tx.clone();
            let mut gas_price = receipt.effective_gas_price.unwrap();
            let mut gas_limit = receipt.gas_used.unwrap();
            gas_price = gas_price + gas_price / 10;
            gas_limit = gas_limit + gas_limit / 10;
            tx = tx.gas(gas_limit).gas_price(gas_price);

            println!("sending to mempool: {:?}", tx);
            let pending_tx = client.send_transaction(tx, None).await?;
            let receipt = pending_tx.await?.ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
            println!("Tx mined, hash: {:?}", receipt.transaction_hash);
            println!("Receipt status: {:?}", receipt.status);

            return Ok(receipt);
        }
    }

    Ok(receipt)
}

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
 * Extract token ID from logs.
 * It is the first parameter of the IncreaseLiquidity event.
 */
fn extract_token_id_from_logs(
    logs: Vec<Log>
) -> Result<U256, Box<dyn std::error::Error>> {
    const EVENT_SIG: &str = "IncreaseLiquidity(uint256,uint128,uint256,uint256)";
    let event_sig_hashed = H256::from(keccak256(EVENT_SIG.as_bytes()));

    for log in logs {
        let topics: Vec<H256> = log.topics;

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
) -> Result<U256, Box<dyn std::error::Error>> {
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

    Ok(balance)
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

    if balance < amount {
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
    println!("Approving {spender} to spend {token_address} MAX U256");
    let function_call = token.approve(
        spender.clone(),
        U256::MAX
    );
    let calldata = function_call.calldata().unwrap();

    let tx = TransactionRequest::new()                                                  
        .to(token_address)                                                              
        .data(calldata);                                                                

    send_tx_to_mempool(
        provider.clone(),
        config.clone(),
        tx
    ).await?;

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
    println!("quote token size desired in USD: {:?}", config.quote_token_size_in_usd);

    // calculate amount of WETH needed based on config.quote_token_size_in_usd
    let inverted = price.clone().invert();
    let quote = inverted.quote(
        CurrencyAmount::from_raw_amount(
            price.quote_currency.clone(),
            config.quote_token_size_in_usd.clone() as u32 * 10u32.pow(6)
        ).unwrap()
    ).unwrap();

    // return amount of WETH needed as a string
    let quote = quote.to_significant(18, Rounding::RoundHalfUp).unwrap();
    let quote = parse_units(quote, 18).unwrap();
    let quote = quote.to_string();

    Ok(quote)
}

async fn swap_token_for_token_given_amount_in(
    provider: Provider<Http>,
    config: Config,
    token_in: Token,
    token_out: Token,
    pool_fee: FeeAmount,
    amount_in: U256,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Swapping {:?} of {:?} for {:?}", amount_in, token_in, token_out);

    // make sure wallet has enough balance of token_in
    assert_balance(
        provider.clone(),
        config.clone(),
        token_in.clone(),
        amount_in.clone()
    ).await?;

    // make sure uniswap router is approved to spend token_in
    erc20_approve(
        provider.clone(),
        config.clone(),
        token_in.clone(),
        config.uniswap_router_address.clone(),
        amount_in.clone()
    ).await?;

    let pool = get_pool(
        config.chain_id,
        config.uniswap_v3_factory_address.parse::<AlloyAddress>()?,
        token_in.address.clone(),
        token_out.address.clone(),
        pool_fee,
        Arc::new(provider.clone()),
        None
    ).await?;

    // create tick provider with data 2% above and below current tick
    let range_ticks = calc_new_ticks(pool.tick_current, config.tick_data_provider_range);
    let lower_tick = nearest_usable_tick(range_ticks.0, pool.tick_spacing());
    let upper_tick = nearest_usable_tick(range_ticks.1, pool.tick_spacing());
    println!("getting tick provider, lower_tick: {:?}, upper_tick: {:?}", lower_tick, upper_tick);
    let tick_provider = EphemeralTickDataProvider::new(
        config.uni_v3_pool_address.clone().parse::<AlloyAddress>()?,
        Arc::new(provider.clone()),
        Some(lower_tick),
        Some(upper_tick),
        None
    ).await?;

    // create pool with tick data provider
    let pool = Pool::new_with_tick_data_provider(
        pool.token0.clone(),
        pool.token1.clone(),
        pool.fee.clone(),
        pool.sqrt_ratio_x96.clone(),
        pool.liquidity.clone(),
        tick_provider
    )?;

    // set deadline 2 minutes from now
    let deadline = (
        SystemTime::now() + Duration::from_secs(2 * 60)
    ).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    let slippage = convert_float_to_percent(config.swap_slippage_pct);
    println!("Preparing swap tx");
    println!("swap slippage: {:?}", slippage);
    println!("");

    // configure swap options
    let options = SwapOptions {
        slippage_tolerance: slippage,
        recipient: config.wallet_address.parse().unwrap(),
        deadline: deadline.to_string().parse().unwrap(),
        input_token_permit: None,
        sqrt_price_limit_x96: None,
        fee: None,
    };

    let trade = Trade::from_route(
        Route::new(vec![pool.clone()], token_in.clone(), token_out.clone()),
        CurrencyAmount::from_raw_amount(
            Currency::Token(token_in.clone()),
            u256_to_big_int(amount_in.to_alloy()),
        )?,
        TradeType::ExactInput,
    )?;

    // prepare the input for router swap
    let method_params = swap_call_parameters(&mut [trade], options.clone()).unwrap();
    let value = convert_to_ethers_u256(method_params.value);

    // prepare tx
    let tx = TransactionRequest::new()
        .to(config.uniswap_router_address.parse::<EthersAddress>()?)
        .value(value)
        .data(method_params.calldata);

    send_tx_to_mempool(
        provider.clone(),
        config.clone(),
        tx
    ).await?;

    Ok(())
}

async fn swap_token_for_token_given_amount_out(
    provider: Provider<Http>,
    config: Config,
    token_in: Token,
    token_out: Token,
    pool_fee: FeeAmount,
    amount_out: U256,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("");
    println!("Swapping token for token for a specific amount out");
    println!("token in: {:?}\ntoken out: {:?}\namount out: {:?}", token_in, token_out, amount_out);
    println!("");
    let pool = get_pool(
        config.chain_id,
        config.uniswap_v3_factory_address.parse::<AlloyAddress>()?,
        token_in.address.clone(),
        token_out.address.clone(),
        pool_fee,
        Arc::new(provider.clone()),
        None
    ).await?;

    let range_ticks = calc_new_ticks(pool.tick_current, config.tick_data_provider_range);
    let lower_tick = nearest_usable_tick(range_ticks.0, pool.tick_spacing());
    let upper_tick = nearest_usable_tick(range_ticks.1, pool.tick_spacing());
    let tick_provider = EphemeralTickDataProvider::new(
        config.uni_v3_pool_address.clone().parse::<AlloyAddress>()?,
        Arc::new(provider.clone()),
        Some(lower_tick),
        Some(upper_tick),
        None
    ).await?;

    let pool = Pool::new_with_tick_data_provider(
        pool.token0.clone(),
        pool.token1.clone(),
        pool.fee.clone(),
        pool.sqrt_ratio_x96.clone(),
        pool.liquidity.clone(),
        tick_provider
    )?;

    // set deadline 2 minutes from now
    let deadline = (
        SystemTime::now() + Duration::from_secs(2 * 60)
    ).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    let slippage = convert_float_to_percent(config.swap_slippage_pct);
    println!("Preparing swap tx");
    println!("swap slippage: {:?}", slippage);
    println!("");

    // configure swap options
    let options = SwapOptions {
        slippage_tolerance: slippage,
        recipient: config.wallet_address.parse().unwrap(),
        deadline: deadline.to_string().parse().unwrap(),
        input_token_permit: None,
        sqrt_price_limit_x96: None,
        fee: None,
    };

    let trade = Trade::from_route(
        Route::new(vec![pool.clone()], token_in.clone(), token_out.clone()),
        CurrencyAmount::from_raw_amount(
            Currency::Token(token_out.clone()),
            u256_to_big_int(amount_out.to_alloy()),
        )?,
        TradeType::ExactOutput,
    )?;
    let amount_in = trade.clone().input_amount().unwrap();
    let amount_in = amount_in.to_exact();
    let amount_in_as_str = amount_in.as_str();
    let amount_in_decimals = token_in.decimals as u32;
    let amount_in = parse_units(amount_in_as_str, amount_in_decimals).unwrap();
    println!("amount in: {:?}", amount_in);
    println!("amount out: {:?}", amount_out);

    // make sure wallet has enough balance of token_in
    assert_balance(
        provider.clone(),
        config.clone(),
        token_in.clone(),
        amount_in.clone().into()
    ).await?;

    // make sure uniswap router is approved to spend token_in
    erc20_approve(
        provider.clone(),
        config.clone(),
        token_in.clone(),
        config.uniswap_router_address.clone(),
        amount_in.clone().into()
    ).await?;

    // prepare the input for router swap
    let method_params = swap_call_parameters(&mut [trade], options.clone()).unwrap();
    let value = convert_to_ethers_u256(method_params.value);

    // prepare tx
    let tx = TransactionRequest::new()
        .to(config.uniswap_router_address.parse::<EthersAddress>()?)
        .value(value)
        .data(method_params.calldata);

    send_tx_to_mempool(
        provider.clone(),
        config.clone(),
        tx
    ).await?;
    
    Ok(())
}

async fn buy_token0_if_needed(
    provider: Provider<Http>,
    config: Config,
    pool: Pool<NoTickDataProvider>,
    token0_amount: U256
) -> Result<(), Box<dyn std::error::Error>> {
    // check balance of token0
    let balance = erc20_balance_of(
        provider.clone(),
        pool.token0.address.to_string(),
        config.wallet_address.clone()
    ).await?;

    // if balance > token1_amount, return
    if balance > token0_amount {
        println!("balance: {:?}, token0_amount: {:?}", balance, token0_amount);
        println!("balance > token0_amount, no need to buy token0");
        return Ok(());
    }

    // calculate the difference between balance and token1_amount
    let balance_diff = token0_amount - balance;

    swap_token_for_token_given_amount_out(
        provider.clone(),
        config.clone(),
        pool.token1.clone(),
        pool.token0.clone(),
        pool.fee.clone(),
        balance_diff,
    ).await?;

    Ok(())
}

async fn buy_token1_if_needed(
    provider: Provider<Http>,
    config: Config,
    pool: Pool<NoTickDataProvider>,
    token1_amount: U256
) -> Result<(), Box<dyn std::error::Error>> {
    // check balance of token1
    let balance = erc20_balance_of(
        provider.clone(),
        pool.token1.address.to_string(),
        config.wallet_address.clone()
    ).await?;

    // if balance > token1_amount, return
    if balance > token1_amount {
        println!("balance: {:?}, token1_amount: {:?}", balance, token1_amount);
        println!("balance > token1_amount, no need to buy token1");
        return Ok(());
    }

    // calculate the difference between balance and token1_amount
    let balance_diff = token1_amount - balance;

    swap_token_for_token_given_amount_out(
        provider.clone(),
        config.clone(),
        pool.token0.clone(),
        pool.token1.clone(),
        pool.fee.clone(),
        balance_diff,
    ).await?;

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
    let mut upper_tick = current_tick + tick_change;
    let mut lower_tick = current_tick - tick_change;

    // make sure the ticks are within the min and max ticks
    if upper_tick > MAX_TICK {
        upper_tick = MAX_TICK;
    }
    if lower_tick < MIN_TICK {
        lower_tick = MIN_TICK;
    }

    (lower_tick, upper_tick)
}

fn convert_to_ethers_u256(alloy_u256: Uint<256, 4>) -> U256 {
    let inner_value: [u8; 32] = alloy_u256.to_le_bytes(); // Convert to bytes in little-endian format
    let mut bytes = [0; 32];
    bytes.copy_from_slice(&inner_value); // Copy the bytes into a fixed-size array

    // Create an ethers U256 from the bytes
    U256::from(bytes)
}

fn convert_float_to_percent(float: f32) -> Percent {
    // increase by constant of 10000
    let constant = 10000.0;
    let numerator = (float * constant) as u64;
    let denominator = (100.0 * constant) as u64;
    Percent::new(numerator, denominator)
}

async fn create_lp_position(
    provider: Provider<Http>,
    config: Config,
    config_file_path: &str
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating new liquidity position.");

    // query v3 pool using address from config file
    let pool = fetch_pool(
        provider.clone(),
        config.clone()
    ).await?;

    // get current price as a tick
    println!("Current tick: {:?}", pool.tick_current);

    // calculate new upper and lower ticks
    let new_ticks = calc_new_ticks(pool.tick_current, config.range_percentage);
    println!("Desired Ticks: {:?}", new_ticks);

    // calculate amounts needed for new position
    let lower_tick = nearest_usable_tick(new_ticks.0, pool.tick_spacing());
    let upper_tick = nearest_usable_tick(new_ticks.1, pool.tick_spacing());
    println!("Desired Usable Ticks: ({:?}, {:?})", lower_tick, upper_tick);
    println!("");

    // get sqrt ratio at the ticks
    let sqrt_ratio_upper = get_sqrt_ratio_at_tick(upper_tick).unwrap();

    // get quote token amount
    let quote_token_amount = 
        if pool.token0.address == config.weth_address.parse::<AlloyAddress>()? ||
            pool.token1.address == config.weth_address.parse::<AlloyAddress>()? {
            get_size_in_weth(provider.clone(), config.clone()).await?
        }
        else if pool.token0.address == config.usdc_address.parse::<AlloyAddress>()? ||
            pool.token1.address == config.usdc_address.parse::<AlloyAddress>()? {
            (config.quote_token_size_in_usd as u32 * 10u32.pow(6)).to_string()
        }
        else { panic!("Neither token0 nor token1 are weth or usdc"); };
    let quote_token_amount = Uint::<256, 4>::from_str_radix(quote_token_amount.as_str(), 10).unwrap();

    // get liquidity for quote token
    let liquidity_for_quote_token =
        // if token0 is quote token, calculate liquidity for token0
        if pool.token0.address == config.weth_address.parse::<AlloyAddress>()? ||
            pool.token0.address == config.usdc_address.parse::<AlloyAddress>()? {
            max_liquidity_for_amount0_imprecise(
                pool.sqrt_ratio_x96.clone(),
                sqrt_ratio_upper.clone(),
                quote_token_amount
            )
        }
        // if token1 is quote token, calculate liquidity for token1
        else if pool.token1.address == config.weth_address.parse::<AlloyAddress>()? ||
            pool.token1.address == config.usdc_address.parse::<AlloyAddress>()? {
            max_liquidity_for_amount1(
                pool.sqrt_ratio_x96.clone(),
                sqrt_ratio_upper.clone(),
                quote_token_amount
            )
        }
        else { panic!("Neither token0 nor token1 are weth or usdc"); }; 

    // create desired position
    let position = Position::new(
        pool.clone(),
        liquidity_for_quote_token.to_u128().unwrap(),
        lower_tick,
        upper_tick
    );

    let mint_amounts = position.clone().mint_amounts()?;
    let token0_amount = mint_amounts.amount0.to_ethers();
    let token1_amount = mint_amounts.amount1.to_ethers();
    println!("");
    println!("token0 amount: {:?}", token0_amount);
    println!("token1 amount: {:?}", token1_amount);
    println!("");

    // if token0 is quote token, buy token1 if needed
    if pool.token0.address == config.weth_address.parse::<AlloyAddress>()? ||
        pool.token0.address == config.usdc_address.parse::<AlloyAddress>()? {
        buy_token1_if_needed(
            provider.clone(),
            config.clone(),
            pool.clone(),
            token1_amount.clone()
        ).await?;
    } 
    // if token1 is quote token, buy token0 if needed
    else if pool.token1.address == config.weth_address.parse::<AlloyAddress>()? ||
        pool.token1.address == config.usdc_address.parse::<AlloyAddress>()? {
        buy_token0_if_needed(
            provider.clone(),
            config.clone(),
            pool.clone(),
            token0_amount.clone()
        ).await?;
    }
    else { panic!("Neither token0 nor token1 are weth or usdc"); }

    // assert sufficient balance of token0
    assert_balance(
        provider.clone(),
        config.clone(),
        pool.token0.clone(),
        token0_amount.clone()
    ).await?;

    // assert sufficient balance of token1
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

    // configure options for adding liquidity to the pool
    let deadline = (
        SystemTime::now() + Duration::from_secs(2 * 60)
    ).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    let slippage = convert_float_to_percent(config.add_liq_slippage_pct);
    println!("");
    println!("Preparing liquidity add tx");
    println!("Allowed slippage: {:?}", slippage);
    println!("");

    // configure add liquidity options
    let options = AddLiquidityOptions {
        slippage_tolerance: slippage,
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

    // prepare the input for nfpm add liquidity method
    let method_params = add_call_parameters(
        &mut position.clone(),
        options
    ).unwrap();

    // prepare tx
    let value = convert_to_ethers_u256(method_params.value);
    let tx = TransactionRequest::new()
        .to(config.uniswap_nfpm_address.parse::<EthersAddress>()?)
        .value(value)
        .data(method_params.calldata);

    // send tx to mempool
    println!("Adding liquidity.");
    let receipt = send_tx_to_mempool(
        provider.clone(),
        config.clone(),
        tx
    ).await?;

    // get token ID from logs in tx receipt
    let token_id = extract_token_id_from_logs(receipt.logs)?;
    println!("New LP token ID: {:?}", token_id);

    // update config file with new LP token ID
    let mut config = config.clone();
    config.my_lp_position_id = token_id.as_usize();
    write_config(config, config_file_path)?;

    Ok(())
}

async fn remove_liquidity_collect_fees(
    provider: Provider<Http>,
    config: Config,
    config_file_path: &str
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Removing liquidity.");

    // get position using token id
    let position = get_position(
        config.chain_id,
        config.uniswap_nfpm_address.parse::<AlloyAddress>()?,
        config.my_lp_position_id.to_string().parse().unwrap(),
        Arc::new(provider.clone()),
        None
    ).await?;

    // check if position is empty
    if position.liquidity == 0 {
        println!("There is no liquidity in the position, has it been removed already?");
        return Ok(());
    }

    // calculate the largest amount of owed token0 and token1 that can be collected
    let burn_amounts = position.clone().burn_amounts_with_slippage(&Percent::new(1, 100)).unwrap();
    let burn_amounts = (u256_to_big_int(burn_amounts.0), u256_to_big_int(burn_amounts.1));
    let currency_owed0_amount = CurrencyAmount::from_raw_amount(
        Currency::Token(position.pool.token0.clone()),
         MAX_UINT256.clone() - burn_amounts.0
    )?;
    let currency_owed1_amount = CurrencyAmount::from_raw_amount(
        Currency::Token(position.pool.token1.clone()),
        MAX_UINT256.clone() - burn_amounts.1
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
    let tx = TransactionRequest::new()
        .to(config.uniswap_nfpm_address.parse::<EthersAddress>()?)
        .value(value)
        .data(method_params.calldata);

    // send tx to mempool
    let receipt = send_tx_to_mempool(
        provider.clone(),
        config.clone(),
        tx
    ).await?;

    // if successful, update config file with new LP token ID as 0
    if receipt.status == Some(U64::from(1)) {
        let mut config = config.clone();
        config.my_lp_position_id = 0;
        write_config(config, config_file_path)?;
    }

    Ok(())
}

async fn sell_leftover_non_quote_token(
    provider: Provider<Http>,
    config: Config,
    lp_position: Position<NoTickDataProvider>
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Selling leftover non-quote token.");

    // if token0 is quote token, sell token1
    if lp_position.pool.token0.address == config.weth_address.parse::<AlloyAddress>()? ||
        lp_position.pool.token0.address == config.usdc_address.parse::<AlloyAddress>()? {

        println!("token0 is quote token, selling token1");
        let token1_bal = erc20_balance_of(
            provider.clone(),
            lp_position.pool.token1.address.to_string(),
            config.wallet_address.clone()
        ).await?;

        swap_token_for_token_given_amount_in(
            provider.clone(),
            config.clone(),
            lp_position.pool.token1.clone(),
            lp_position.pool.token0.clone(),
            lp_position.pool.fee.clone(),
            token1_bal
        ).await?;
    } 
    // if token1 is quote token, sell token0
    else if lp_position.pool.token1.address == config.weth_address.parse::<AlloyAddress>()? ||
        lp_position.pool.token1.address == config.usdc_address.parse::<AlloyAddress>()? {

        println!("token1 is quote token, selling token0");
        let token0_bal = erc20_balance_of(
            provider.clone(),
            lp_position.pool.token0.address.to_string(),
            config.wallet_address.clone()
        ).await?;

        swap_token_for_token_given_amount_in(
            provider.clone(),
            config.clone(),
            lp_position.pool.token0.clone(),
            lp_position.pool.token1.clone(),
            lp_position.pool.fee.clone(),
            token0_bal
        ).await?;
    }
    else { panic!("Neither token0 nor token1 are weth or usdc"); }

    Ok(())
}

async fn adjust_lower(
    provider: Provider<Http>,
    config: Config,
    lp_position: Position<NoTickDataProvider>,
    config_file_path: &str
) -> Result<(), Box<dyn std::error::Error>> {
    // undo the current LP position and collect fees
    remove_liquidity_collect_fees(
        provider.clone(),
        config.clone(),
        config_file_path
    ).await?;

    // create new LP position and save its token ID to config file
    create_lp_position(
        provider.clone(),
        config.clone(),
        config_file_path
    ).await?;

    // sell any leftover of the non-quote token
    sell_leftover_non_quote_token(
        provider.clone(),
        config.clone(),
        lp_position.clone()
    ).await?;

    Ok(())
}

async fn adjust_higher(
    provider: Provider<Http>,
    config: Config,
    config_file_path: &str
) -> Result<(), Box<dyn std::error::Error>> {
    // undo the current LP position and collect fees
    remove_liquidity_collect_fees(
        provider.clone(),
        config.clone(),
        config_file_path
    ).await?;

    // create new LP position and save its token ID to config file
    create_lp_position(
        provider.clone(),
        config.clone(),
        config_file_path
    ).await?;

    Ok(())
}

fn print_lp_position_details(lp_position: &Position<NoTickDataProvider>) {
    println!("LP Position");
    println!("-----------");
    println!(
        "Symbol:\t\t\t{:?}/{:?}",
        lp_position.pool.token1.symbol.clone().unwrap(),
        lp_position.pool.token0.symbol.clone().unwrap()
    );
    println!("Lower Tick:\t\t{:?}", lp_position.tick_lower);
    println!("Upper Tick:\t\t{:?}", lp_position.tick_upper);
    println!("Current Tick:\t\t{:?}", lp_position.pool.tick_current);
    println!("Our Liquidity:\t\t{:?}", lp_position.liquidity);
    println!("Pool Liquidity:\t\t{:?}", lp_position.pool.liquidity);
    println!("TODO: calculate total liquidity in the range instead of entire pool.");
    println!("");
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
    let mut config = read_config(config_file_path)?;

    // Set up ethers provider
    let provider = Provider::<Http>::try_from(
        config.ethers_provider_url.clone(),
    )?;
    let block_number = provider.get_block_number().await?;
    println!("Block Number: {block_number}");
    println!("");

    loop {
        // reload config file
        config = read_config(config_file_path)?;

        // if my_lp_position_id is 0, then create a new LP position
        // and save the ID to the config file
        if config.my_lp_position_id == 0 {
            println!("no LP position ID found in config file, creating new LP position");
            create_lp_position(
                provider.clone(),
                config.clone(),
                config_file_path
            ).await?;

            // reload config file
            config = read_config(config_file_path)?;
        }

        // get current LP position based on ID from config file
        let lp_position = get_position(
            config.chain_id.clone(),
            config.uniswap_nfpm_address.clone().parse::<AlloyAddress>()?,
            config.my_lp_position_id.clone().to_string().parse().unwrap(),
            Arc::new(provider.clone()),
            None
        ).await?;
        print_lp_position_details(&lp_position);

        // check if current tick is outside of our position range
        // and adjust the position as necessary
        let current_tick = lp_position.pool.tick_current.clone();
        if current_tick < lp_position.tick_lower {
            println!("current tick is lower than our LP price range, adjusting lower.");
            println!("");
            adjust_lower(
                provider.clone(),
                config.clone(),
                lp_position.clone(),
                config_file_path
            ).await?;
        }
        else if current_tick > lp_position.tick_upper {
            println!("current tick is higher than our LP price range, adjusting higher.");
            println!("");
            adjust_higher(
                provider.clone(),
                config.clone(),
                config_file_path
            ).await?;
        }
        else {
            println!("current tick is within our LP price range");
        }

        // wait 5 minutes
        println!("waiting for {} seconds", config.seconds_to_wait);
        println!("");
        std::thread::sleep(Duration::from_secs(config.seconds_to_wait as u64));
    }

    Ok(())
}
