// src/multi.rs

use ethers::{
    abi::Abi,
    prelude::*,
    types::{H160, U256},
};
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct Reserve {
    pub reserve0: U256,
    pub reserve1: U256,
}

pub async fn batch_get_uniswap_v2_reserves(
    provider: Arc<Provider<Http>>,
    pool_addresses: Vec<H160>,
) -> Result<HashMap<H160, Reserve>, anyhow::Error> {
    
    if pool_addresses.is_empty() {
        return Ok(HashMap::new());
    }

    let pair_abi: Abi = serde_json::from_str(
        r#"[{"inputs":[],"name":"getReserves","outputs":[{"internalType":"uint112","name":"_reserve0","type":"uint112"},{"internalType":"uint112","name":"_reserve1","type":"uint112"},{"internalType":"uint32","name":"_blockTimestampLast","type":"uint32"}],"stateMutability":"view","type":"function"}]"#,
    )?;

    let mut multicall = Multicall::new(provider.clone(), None).await?;

    for pool_address in &pool_addresses {
        let contract = Contract::new(*pool_address, pair_abi.clone(), provider.clone());
        multicall.add_call(contract.method::<_, (u128, u128, u32)>("getReserves", ())?, false);
    }

    let results = multicall.call_raw().await?;
    
    let mut reserves_map = HashMap::new();
    for (i, result) in results.into_iter().enumerate() {
        if let Ok(token) = result {
            if let Some(res_tuple) = token.into_tuple() {
                if res_tuple.len() >= 2 {
                    let reserve0: U256 = res_tuple[0].clone().into_uint().unwrap_or_default();
                    let reserve1: U256 = res_tuple[1].clone().into_uint().unwrap_or_default();
                    
                    let pool_address = pool_addresses[i];
                    reserves_map.insert(
                        pool_address,
                        Reserve {
                            reserve0,
                            reserve1,
                        },
                    );
                }
            }
        } else {
            warn!("La llamada a getReserves falló (revert) para el pool: {:?}", pool_addresses[i]);
        }
    }

    Ok(reserves_map)
}
