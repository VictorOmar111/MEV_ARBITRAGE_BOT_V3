use anyhow::Result;
use ethers::{
    prelude::*,
    types::{H160, U256},
};
use std::{collections::HashMap, sync::Arc};

// ABIs para los contratos con los que interactuaremos en el multicall.
abigen!(IUniswapV3Pool, "./abi/IUniswapV3Pool.json");
abigen!(IERC20, "./abi/IERC20.json");

/// Estructura para contener los datos brutos de un pool obtenidos del multicall.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawPoolData {
    pub factory: H160,
    pub token0: H160,
    pub token1: H160,
    pub decimals0: u8,
    pub decimals1: u8,
    pub liquidity: u128,
    pub sqrt_price_x96: U256,
    pub fee: u32,
    pub balance0: U256,
    pub balance1: U256,
}

/// Obtiene los datos esenciales de una lista de pools V3 usando multicall.
pub async fn batch_get_pool_data<M: Middleware + 'static>(
    provider: Arc<M>,
    pool_addresses: &[H160],
) -> Result<HashMap<H160, RawPoolData>> {
    let mut multicall = Multicall::new(provider.clone(), None).await?;

    // --- 1. Primera Pasada: Obtener datos principales de los pools ---
    for &addr in pool_addresses {
        let pool_contract = IUniswapV3Pool::new(addr, provider.clone());
        multicall.add_call(pool_contract.factory(), true);
        multicall.add_call(pool_contract.token_0(), true);
        multicall.add_call(pool_contract.token_1(), true);
        multicall.add_call(pool_contract.liquidity(), true);
        multicall.add_call(pool_contract.slot_0(), true);
        multicall.add_call(pool_contract.fee(), true);
    }
    let results_pools = multicall.call_raw().await?;
    multicall.clear_calls();

    let mut intermediate_data = HashMap::new();
    let mut token_contracts = HashMap::new();
    let num_calls_per_pool = 6;

    for (i, &addr) in pool_addresses.iter().enumerate() {
        let start_idx = i * num_calls_per_pool;
        if results_pools[start_idx].is_ok() {
            let factory: H160 = results_pools[start_idx].clone().unwrap().into_address().unwrap_or_default();
            let token0: H160 = results_pools[start_idx + 1].clone().unwrap().into_address().unwrap_or_default();
            let token1: H160 = results_pools[start_idx + 2].clone().unwrap().into_address().unwrap_or_default();
            let liquidity: u128 = results_pools[start_idx + 3].clone().unwrap().into_uint().unwrap_or_default().as_u128();
            let slot0_tokens = results_pools[start_idx + 4].clone().unwrap().into_tuple().unwrap_or_default();
            let sqrt_price_x96 = slot0_tokens.get(0).and_then(|t| t.clone().into_uint()).unwrap_or_default();
            let fee: u32 = results_pools[start_idx + 5].clone().unwrap().into_uint().unwrap_or_default().as_u32();

            if !token0.is_zero() && !token1.is_zero() {
                intermediate_data.insert(addr, (factory, token0, token1, liquidity, sqrt_price_x96, fee));
                token_contracts.entry(token0).or_insert_with(|| IERC20::new(token0, provider.clone()));
                token_contracts.entry(token1).or_insert_with(|| IERC20::new(token1, provider.clone()));
            }
        }
    }

    // --- 2. Segunda Pasada: Obtener decimales de los tokens únicos ---
    let unique_tokens: Vec<H160> = token_contracts.keys().cloned().collect();
    for &token_addr in &unique_tokens {
        multicall.add_call(token_contracts.get(&token_addr).unwrap().decimals(), true);
    }
    let results_decimals = multicall.call_raw().await?;
    multicall.clear_calls();

    let mut token_decimals: HashMap<H160, u8> = HashMap::new();
    for (i, &token_addr) in unique_tokens.iter().enumerate() {
        if let Ok(decimals_token) = &results_decimals[i] {
            token_decimals.insert(token_addr, decimals_token.clone().into_uint().unwrap_or_default().as_u32() as u8);
        } else {
            token_decimals.insert(token_addr, 18); // Default a 18 si la llamada falla
        }
    }

    // --- 3. Tercera Pasada: Obtener balances de los pools ---
    for (pool_addr, (_, token0, token1, _, _, _)) in &intermediate_data {
        multicall.add_call(token_contracts.get(token0).unwrap().balance_of(*pool_addr), true);
        multicall.add_call(token_contracts.get(token1).unwrap().balance_of(*pool_addr), true);
    }
    let results_balances = multicall.call_raw().await?;

    // --- 4. Ensamblaje Final ---
    let mut final_reserves = HashMap::new();
    let mut balance_idx = 0;
    for (pool_addr, (factory, token0, token1, liquidity, sqrt_price_x96, fee)) in intermediate_data {
        let balance0 = results_balances.get(balance_idx).and_then(|r| r.as_ref().ok()).and_then(|t| t.clone().into_uint()).unwrap_or_default();
        let balance1 = results_balances.get(balance_idx + 1).and_then(|r| r.as_ref().ok()).and_then(|t| t.clone().into_uint()).unwrap_or_default();
        balance_idx += 2;

        final_reserves.insert(pool_addr, RawPoolData {
            factory, token0, token1,
            decimals0: token_decimals.get(&token0).cloned().unwrap_or(18),
            decimals1: token_decimals.get(&token1).cloned().unwrap_or(18),
            liquidity, sqrt_price_x96, fee, balance0, balance1
        });
    }

    Ok(final_reserves)
}
