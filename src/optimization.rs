use crate::execution;
use futures::future::join_all;
use crate::{
    config::CONFIG,
    oracle::OracleMap,
    paths::ArbPath,
    types::{OraclePriceInfo, Pool},
    constants::WETH_ADDRESS,
};
use anyhow::{anyhow, Result};
use ethers::{
    prelude::*,
    types::{H160, U256},
};
use lazy_static::lazy_static;
use rust_decimal::{prelude::*, Decimal, MathematicalOps};
use std::{
    collections::{HashMap, VecDeque},
    str::FromStr,
    sync::{Arc, Mutex},
};

#[derive(Debug, Default, Clone)]
pub struct RouteHistory {
    pub successes: u64,
    pub failures: u64,
    pub last_attempt_block: u64,
    pub last_failure_block: u64,
}
impl RouteHistory {
    pub fn winrate(&self) -> f64 {
        let total = self.successes + self.failures;
        if total == 0 { 0.5 } else { self.successes as f64 / total as f64 }
    }
}
lazy_static! {
    pub static ref ROUTE_STATS: Mutex<HashMap<String, RouteHistory>> = Mutex::new(HashMap::new());
}

pub fn u256_to_decimal(val: U256, decimals: u8) -> Result<Decimal> {
    Decimal::from_str(&val.to_string())?.checked_div(Decimal::from(10u128.pow(decimals as u32))).ok_or_else(|| anyhow!("division por cero"))
}
pub fn decimal_to_u256(val: Decimal, decimals: u8) -> Result<U256> {
    let scaled = val * Decimal::from(10u128.pow(decimals as u32));
    U256::from_str(&scaled.round().to_string()).map_err(|e| anyhow!("error parseando U256: {e}"))
}
#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub path: ArbPath,
    pub optimal_amount_in: U256,
    pub expected_output: U256,
    pub net_profit_usd: f64,
    pub bribe_usd: f64,
    pub lag: f64,
    pub tvl: f64,
    pub score: f64,
    pub slippage_bps: u32,
}
async fn get_profit_for_amount<M: Middleware + 'static>(
    provider: &Arc<M>, path: &ArbPath, amount_in: U256, base_gas_price_wei: U256, oracle_price_usd: f64, eth_price_usd: f64,
) -> f64 {
    if amount_in.is_zero() || oracle_price_usd <= 0.0 || eth_price_usd <= 0.0 { return -1.0; }
    let gross_amount_out = match path.simulate_v3_path(provider.clone(), amount_in).await {
        Some(out) if out > amount_in => out,
        _ => return -1.0,
    };
    let gross_profit_u256 = gross_amount_out - amount_in;
    let gross_profit_dec = u256_to_decimal(gross_profit_u256, path.get_input_decimals()).unwrap_or_default();
    let gross_profit_usd = gross_profit_dec.to_f64().unwrap_or(0.0) * oracle_price_usd;
    let bribe_usd = gross_profit_usd * CONFIG.max_bribe_percent;
    let bribe_eth = bribe_usd / eth_price_usd;
    let priority_fee_wei = decimal_to_u256(Decimal::from_f64(bribe_eth).unwrap_or_default(), 18).unwrap_or_default();
    let total_gas_price = base_gas_price_wei + priority_fee_wei;
    let gas_cost_eth = u256_to_decimal(total_gas_price * U256::from(CONFIG.gas_limit), 18).unwrap_or_default();
    let gas_cost_usd = gas_cost_eth.to_f64().unwrap_or(0.0) * eth_price_usd;
    gross_profit_usd - gas_cost_usd
}
pub async fn find_best_trade_golden_section<M: Middleware + 'static>(
    provider: Arc<M>, path: &mut ArbPath, base_gas_price_wei: U256, oracle_info: OraclePriceInfo, oracle_map: &Arc<OracleMap>, current_block: u64,
) -> Option<ArbitrageOpportunity> {
    let (mut a, mut b, tol) = (U256::from(10).pow(17.into()), U256::from(10).pow(20.into()), U256::from(10).pow(15.into()));
    let eth_price = oracle_map.get_price(&*WETH_ADDRESS, provider.clone()).await?.price;
    let oracle_price = oracle_info.price;
    let lag = oracle_info.lag;
    let gr = (Decimal::from(5).sqrt().unwrap() - Decimal::ONE) / Decimal::TWO;
    let gr_u256 = decimal_to_u256(gr, 18).ok()?;
    let mut x1 = a + (b - a) * (U256::exp10(18) - gr_u256) / U256::exp10(18);
    let mut x2 = a + (b - a) * gr_u256 / U256::exp10(18);
    let mut f1 = get_profit_for_amount(&provider, path, x1, base_gas_price_wei, oracle_price, eth_price).await;
    let mut f2 = get_profit_for_amount(&provider, path, x2, base_gas_price_wei, oracle_price, eth_price).await;
    for _ in 0..15 {
        if (b - a) <= tol { break; }
        if f1 > f2 {
            b = x2; x2 = x1; f2 = f1;
            x1 = a + (b - a) * (U256::exp10(18) - gr_u256) / U256::exp10(18);
            f1 = get_profit_for_amount(&provider, path, x1, base_gas_price_wei, oracle_price, eth_price).await;
        } else {
            a = x1; x1 = x2; f1 = f2;
            x2 = a + (b - a) * gr_u256 / U256::exp10(18);
            f2 = get_profit_for_amount(&provider, path, x2, base_gas_price_wei, oracle_price, eth_price).await;
        }
    }
    let optimal_amount = (a + b) / 2;
    let net_profit_usd = f1.max(f2);
    if net_profit_usd <= CONFIG.min_profit_usd { return None; }
    let expected_output = path.simulate_v3_path(provider, optimal_amount).await.unwrap_or_default();
    let path_key = path.key();
    let mut stats_map = ROUTE_STATS.lock().unwrap();
    let stats = stats_map.entry(path_key).or_default();
    stats.last_attempt_block = current_block;
    let total_fee_bps = (path.pool_1.fee + path.pool_2.fee + path.pool_3.fee) as f64;
    let fee_efficiency = 1.0 / (1.0 + total_fee_bps / 10000.0);
    let tvl_avg = (path.pool_1.tvl_usd + path.pool_2.tvl_usd + path.pool_3.tvl_usd) / 3.0;
    let score = net_profit_usd * (1.0 + lag) * stats.winrate() * fee_efficiency * tvl_avg.log10().max(1.0);
    path.score = score;
    let gas_cost_usd_estimate = (eth_price * u256_to_decimal(base_gas_price_wei * CONFIG.gas_limit, 18).unwrap_or_default().to_f64().unwrap_or_default());
    let gross_profit_usd = net_profit_usd + gas_cost_usd_estimate;
    let bribe_usd = gross_profit_usd * CONFIG.max_bribe_percent;
    Some(ArbitrageOpportunity {
        path: path.clone(), optimal_amount_in: optimal_amount, expected_output, net_profit_usd,
        bribe_usd, lag, tvl: tvl_avg, score, slippage_bps: 0,
    })
}
