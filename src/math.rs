// src/math.rs

use ethers::contract::abigen;
use ethers::middleware::Middleware;
use ethers::providers::{Http, Provider};
use ethers::types::{H160, U256};
use once_cell::sync::Lazy;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::{Decimal, MathematicalOps};
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::config::CONFIG;
use crate::multi::Reserve;
use crate::paths::ArbPath;
use crate::provider::get_eth_price;

abigen!(
    AggregatorV3Interface,
    r#"[
        function latestRoundData() view returns (uint80, int256, uint256, uint256, uint80)
        function decimals() view returns (uint8)
    ]"#
);

static DECIMALS_CACHE: Lazy<Mutex<HashMap<H160, u32>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const UNISWAP_V2_FEE: Decimal = dec!(0.997);

pub fn u256_to_decimal(val: &U256, decimals: u32) -> Decimal {
    Decimal::from_str(&val.to_string()).unwrap_or(Decimal::ZERO)
        / Decimal::from(10u64.pow(decimals))
}

pub async fn get_token_decimals(provider: Arc<Provider<Http>>, token: H160) -> u32 {
    {
        let cache = DECIMALS_CACHE.lock().unwrap();
        if let Some(&dec) = cache.get(&token) { return dec; }
    }
    abigen!(
        ERC20,
        r#"[
            function decimals() view returns (uint8)
        ]"#
    );
    let contract = ERC20::new(token, provider.clone());
    let decimals = contract.decimals().call().await.unwrap_or(18u8) as u32;
    DECIMALS_CACHE.lock().unwrap().insert(token, decimals);
    decimals
}

pub async fn calculate_optimal_input(
    path: &ArbPath,
    reserves: &HashMap<H160, Reserve>,
) -> Result<Option<U256>, String> {
    let reserve1 = reserves.get(&path.pool_1.address).ok_or("Reserve for pool_1 not found")?;
    let reserve2 = reserves.get(&path.pool_2.address).ok_or("Reserve for pool_2 not found")?;
    let reserve3 = reserves.get(&path.pool_3.address).ok_or("Reserve for pool_3 not found")?;

    let (res_a_in, res_a_out) = if path.zero_for_one_1 { (reserve1.reserve0, reserve1.reserve1) } else { (reserve1.reserve1, reserve1.reserve0) };
    let (res_b_in, res_b_out) = if path.zero_for_one_2 { (reserve2.reserve0, reserve2.reserve1) } else { (reserve2.reserve1, reserve2.reserve0) };
    let (res_c_in, res_c_out) = if path.zero_for_one_3 { (reserve3.reserve0, reserve3.reserve1) } else { (reserve3.reserve1, reserve3.reserve0) };
    
    // Simulación simple para determinar si hay profit bruto.
    // Una implementación completa usaría la fórmula analítica para el input óptimo.
    let amount_in_sim = U256::exp10(18); // Simular con 1 token
    if let Some(amount_out_sim) = path.simulate_v2_path(amount_in_sim, reserves) {
        if amount_out_sim > amount_in_sim {
            // Si hay profit, devolvemos un valor fijo como placeholder del óptimo.
            // En una implementación real, aquí iría la fórmula matemática compleja.
            return Ok(Some(amount_in_sim));
        }
    }

    Ok(None)
}

pub async fn calculate_net_profit(
    provider: Arc<Provider<Http>>,
    amount_in: U256,
    path: &ArbPath,
    reserves: &HashMap<H160, Reserve>,
) -> Result<(f64, U256), String> {
    let amount_in_decimals = get_token_decimals(provider.clone(), path.token_a).await;

    let (gross_profit_u256, expected_output) = match path.simulate_v2_path(amount_in, reserves) {
        Some(amount_out) if amount_out > amount_in => (amount_out - amount_in, amount_out),
        _ => return Ok((0.0, U256::zero())),
    };

    let gross_profit_dec = u256_to_decimal(&gross_profit_u256, amount_in_decimals);

    let eth_price_usd = get_eth_price().await;
    let token_price_usd = get_token_price_in_usd(provider.clone(), path.token_a).await?;
    let gross_profit_usd = gross_profit_dec.to_f64().unwrap_or(0.0) * token_price_usd;
    let gross_profit_eth = Decimal::from_f64(gross_profit_usd / eth_price_usd).unwrap_or(Decimal::ZERO);

    let (max_fee_per_gas, _) = provider.estimate_eip1559_fees(None).await.map_err(|e| e.to_string())?;
    let gas_price_eth = u256_to_decimal(&max_fee_per_gas, 18);
    
    let gas_used = dec!(250000); 
    let gas_cost_eth = gas_used * gas_price_eth;

    let bribe_percent = Decimal::from_f64(CONFIG.bribe_percent).unwrap_or(dec!(0)) / dec!(100);
    let bribe_eth = gross_profit_eth * bribe_percent;

    let total_cost_eth = gas_cost_eth + bribe_eth;
    let net_profit_eth = gross_profit_eth - total_cost_eth;
    let net_profit_usd = net_profit_eth.to_f64().unwrap_or(0.0) * eth_price_usd;

    Ok((net_profit_usd, expected_output))
}

pub async fn get_token_price_in_usd(
    provider: Arc<Provider<Http>>,
    token: H160,
) -> Result<f64, String> {
    let weth: H160 = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap();
    if token == weth {
        return Ok(get_eth_price().await);
    }
    // ... Lógica para obtener precios de stablecoins y Chainlink ...
    Ok(1.0) // Placeholder
      }
  
