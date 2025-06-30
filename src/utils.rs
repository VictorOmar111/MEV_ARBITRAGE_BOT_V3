// src/utils.rs

use anyhow::Result;
use chrono::Local;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::{BlockId, H160, U256, U64},
};
use fern::colors::{Color, ColoredLevelConfig};
use log::LevelFilter;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::multi::{batch_get_uniswap_v2_reserves, Reserve};

/// Configura un logger simple y colorido para la terminal.
pub fn setup_logger() -> Result<()> {
    let colors = ColoredLevelConfig::new()
        .info(Color::Green)
        .warn(Color::Yellow)
        .error(Color::Red)
        .debug(Color::White)
        .trace(Color::BrightBlack);

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{time}][{level}] {message}",
                time = Local::now().format("%H:%M:%S"),
                level = colors.color(record.level()),
                message = message,
            ));
        })
        .level(LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()?;

    Ok(())
}

/// Calcula el `base_fee` del siguiente bloque según las reglas de EIP-1559.
pub fn calculate_next_block_base_fee(
    gas_used: U256,
    gas_limit: U256,
    base_fee: U256,
) -> U256 {
    let gas_target = gas_limit / 2;
    if gas_used == gas_target {
        return base_fee;
    }
    
    let base_fee_delta = (base_fee * (gas_used - gas_target) / gas_target) / 8;
    base_fee + base_fee_delta
}


pub async fn get_touched_pool_reserves(
    provider: Arc<Provider<Http>>,
    block_number: U64,
) -> Result<HashMap<H160, Reserve>> {
    let block = provider
        .get_block_with_txs(BlockId::Number(block_number.into()))
        .await?
        .ok_or_else(|| anyhow::anyhow!("Block not found: {}", block_number))?;

    let mut touched_pools = HashSet::new();
    for tx in block.transactions {
        if let Some(to) = tx.to {
            touched_pools.insert(to);
        }
    }

    let pool_addresses: Vec<H160> = touched_pools.into_iter().collect();
    if pool_addresses.is_empty() {
        return Ok(HashMap::new());
    }

    batch_get_uniswap_v2_reserves(provider, pool_addresses).await
}
