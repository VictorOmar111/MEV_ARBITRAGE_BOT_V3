// src/strategy.rs

use ethers::{
    prelude::*,
    types::{H160, U256},
};
use log::{info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;

use crate::config::CONFIG;
use crate::constants::Env;
use crate::execution::execute_flashloan_transaction;
use crate::math::{calculate_net_profit, calculate_optimal_input};
use crate::multi::Reserve;
use crate::paths::{generate_triangular_paths, ArbPath};
use crate::pools::{load_all_pools_from_v2, Pool};
use crate::streams::Event;
use crate::utils::get_touched_pool_reserves;

#[derive(Clone, Debug)]
pub struct ArbitrageOpportunity {
    pub path: ArbPath,
    pub optimal_amount_in: U256,
    pub net_profit_usd: f64,
    pub expected_output: U256,
}

pub async fn event_handler(
    provider_ws: Arc<Provider<Ws>>,
    event_sender: Sender<Event>,
) -> anyhow::Result<()> {
    let env = Env::new();
    let http_provider = Arc::new(Provider::<Http>::try_from(CONFIG.https_url.clone())?);

    let factory_addresses = vec![
        env.factory_address.parse::<H160>()?, // Uniswap V2
        "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac".parse::<H160>()?, // Sushiswap Factory
        "0x115934131916C8b277DD010Ee02de363c09d037c".parse::<H160>()?, // ShibaSwap Factory
    ];
    
    let factory_creation_blocks = vec![
        env.factory_creation_block, // Uniswap V2
        10794229, // Sushiswap
        12767228, // ShibaSwap
    ];

    let pools = load_all_pools_from_v2(env.wss_url.clone(), factory_addresses, factory_creation_blocks).await?;
    let token_in: H160 = env.token_in.parse()?;
    let mut event_receiver = event_sender.subscribe();

    info!("Strategy handler started. Monitoring Uniswap, Sushiswap & ShibaSwap...");

    loop {
        if let Ok(event) = event_receiver.recv().await {
            if let Event::Block(block) = event {
                let reserves: HashMap<H160, Reserve> = get_touched_pool_reserves(http_provider.clone(), block.block_number)
                    .await
                    .unwrap_or_default();
                
                if reserves.is_empty() { continue; }

                let paths = generate_triangular_paths(&pools, token_in);
                let mut best_opportunity: Option<ArbitrageOpportunity> = None;

                for path in &paths {
                    if let Ok(Some(optimal_amount_in)) = calculate_optimal_input(path, &reserves).await {
                        if !optimal_amount_in.is_zero() {
                            if let Ok((net_profit_usd, expected_output)) = calculate_net_profit(http_provider.clone(), optimal_amount_in, path, &reserves).await {
                                let is_better = best_opportunity.as_ref().map_or(true, |best| net_profit_usd > best.net_profit_usd);

                                if net_profit_usd > CONFIG.min_profit_usd && is_better {
                                    info!("Found new best opportunity! Profit: ${:.2}, Path: {:?}", net_profit_usd, path.get_swap_path());
                                    best_opportunity = Some(ArbitrageOpportunity {
                                        path: path.clone(),
                                        optimal_amount_in,
                                        net_profit_usd,
                                        expected_output,
                                    });
                                }
                            }
                        }
                    }
                }

                if let Some(winner) = best_opportunity {
                    info!("Executing best opportunity! Profit: ${:.2}, Amount In: {}", winner.net_profit_usd, winner.optimal_amount_in);
                    if let Err(e) = execute_flashloan_transaction(&winner.path, winner.optimal_amount_in, winner.expected_output).await {
                        warn!("Error executing arbitrage transaction: {:?}", e);
                    }
                }
            }
        }
    }
                                  }
