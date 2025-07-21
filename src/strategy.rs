use crate::{
    config::CONFIG,
    constants::{PANCAKESWAP_V3_FACTORY, SUSHISWAP_V3_FACTORY, UNISWAP_V3_FACTORY},
    execution,
    optimization::{self, ArbitrageOpportunity, ROUTE_STATS},
    oracle::{self, OracleMap},
    paths::{self, generate_triangular_paths, ArbPath},
    pools,
    streams::Event,
    types::{DexVariant, Pool}, // Importación directa de Pool
};
use ethers::{prelude::*, types::U256};
use futures_util::{stream::FuturesUnordered, StreamExt};
use lazy_static::lazy_static;
use log::{info, warn};
use prometheus::{register_int_counter, register_int_gauge, IntCounter, IntGauge};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::broadcast::Sender;

lazy_static! {
    static ref ROUTES_EVALUATED: IntCounter = register_int_counter!("routes_evaluated_total", "Total de rutas evaluadas").unwrap();
    static ref TRADES_EXECUTED: IntCounter = register_int_counter!("trades_executed_total", "Total de trades enviados").unwrap();
    static ref TRADES_FAILED: IntCounter = register_int_counter!("trades_failed_total", "Total de trades que fallaron").unwrap();
    static ref CURRENT_PATHS: IntGauge = register_int_gauge!("current_paths_available", "Rutas de arbitraje disponibles").unwrap();
}

const OPPORTUNITY_BUNDLE_SIZE: usize = 5;
const ROUTE_FAILURE_COOLDOWN_BLOCKS: u64 = 10;

// CORRECCIÓN FINAL: La firma ahora coincide perfectamente con el tipo de `client` creado en `lib.rs`
pub async fn event_handler(
    client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    provider_ws: Arc<Provider<Ws>>,
    oracle_map: Arc<OracleMap>,
    event_sender: Sender<Event>,
    initial_pools: Vec<Pool>, // Usamos `Pool` directamente desde `types`
    initial_paths: Vec<ArbPath>,
) -> anyhow::Result<()> {

    let _dex_factories = vec![
        (*UNISWAP_V3_FACTORY, 420, DexVariant::UniswapV3),
        (*SUSHISWAP_V3_FACTORY, 19620263, DexVariant::SushiV3),
        (*PANCAKESWAP_V3_FACTORY, 61748453, DexVariant::PancakeV3),
    ];

    let mut pools = initial_pools;
    let mut paths = initial_paths;
    CURRENT_PATHS.set(paths.len() as i64);

    let mut event_receiver = event_sender.subscribe();
    let mut last_refresh_block = 0u64;
    info!(" Estrategia lista con {} rutas. Esperando nuevos bloques...", paths.len());

    loop {
        if let Ok(Event::Block(block)) = event_receiver.recv().await {
            let block_number = block.number.unwrap_or_default().as_u64();
            info!("--- Bloque Nuevo #{block_number} ---");

            if last_refresh_block == 0
                || block_number.saturating_sub(last_refresh_block)
                    >= CONFIG.path_refresh_interval_blocks
            {
                info!(" Refrescando lista de pools y rutas...");
                pools = pools::load_all_pools_v3(provider_ws.clone(), &oracle_map).await?;
                paths = generate_triangular_paths(&pools, CONFIG.token_in_address, &oracle_map);
                CURRENT_PATHS.set(paths.len() as i64);
                last_refresh_block = block_number;
                crate::clear_old_locks(block_number);
            }

            let base_gas_price = block.base_fee_per_gas.unwrap_or_else(U256::zero);
            let tasks = FuturesUnordered::new();

            for path in &paths {
                let is_in_cooldown = {
                    let stats_map = ROUTE_STATS.lock().unwrap();
                    if let Some(stats) = stats_map.get(&path.key()) {
                        block_number < stats.last_failure_block + ROUTE_FAILURE_COOLDOWN_BLOCKS
                    } else {
                        false
                    }
                };
                if is_in_cooldown { continue; }

                let mut p = path.clone();
                let prov = Arc::new(client.provider().clone());
                let omap = oracle_map.clone();
                tasks.push(tokio::spawn(async move {
                    ROUTES_EVALUATED.inc();
                    let spot_price = p.get_spot_price(prov.clone()).await.ok()?;
                    let oracle_info =
                        oracle::get_max_profit_oracle(&p.token_a, spot_price, &omap, prov.clone())
                            .await?;
                    optimization::find_best_trade_golden_section(
                        prov, &mut p, base_gas_price, oracle_info, &omap, block_number,
                    ).await
                }));
            }

            let mut profitable_opportunities: Vec<ArbitrageOpportunity> =
                tasks.filter_map(|res| async { res.ok().flatten() }).collect().await;

            if profitable_opportunities.is_empty() {
                info!("No se encontraron oportunidades rentables en este bloque.");
                continue;
            }

            profitable_opportunities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

            let mut bundle_to_execute = Vec::new();
            let mut used_pools = HashSet::new();

            for opp in profitable_opportunities {
                if bundle_to_execute.len() >= OPPORTUNITY_BUNDLE_SIZE { break; }

                let p1 = opp.path.address(1);
                let p2 = opp.path.address(2);
                let p3 = opp.path.address(3);
                if used_pools.contains(&p1) || used_pools.contains(&p2) || used_pools.contains(&p3) { continue; }

                let mut final_opp = opp.clone();
                final_opp.slippage_bps = calculate_dynamic_slippage(opp.tvl, opp.net_profit_usd);

                if crate::lock_opportunity(block_number, &final_opp.path) {
                    used_pools.insert(p1);
                    used_pools.insert(p2);
                    used_pools.insert(p3);
                    bundle_to_execute.push(final_opp);
                }
            }

            if !bundle_to_execute.is_empty() {
                let execution_results = execution::execute_arbitrage_bundle(
                    client.clone(), bundle_to_execute, base_gas_price,
                ).await;
                for result in execution_results {
                    match result {
                        Ok((_tx_hash, path_key)) => {
                            TRADES_EXECUTED.inc();
                            let mut stats_map = ROUTE_STATS.lock().unwrap();
                            let stats = stats_map.entry(path_key).or_default();
                            stats.successes += 1;
                        }
                        Err((e, path_key)) => {
                            TRADES_FAILED.inc();
                            let mut stats_map = ROUTE_STATS.lock().unwrap();
                            let stats = stats_map.entry(path_key.clone()).or_default();
                            stats.failures += 1;
                            stats.last_failure_block = block_number;
                            warn!(" Falló TX del bundle para la ruta {path_key}: {e:?}");
                        }
                    }
                }
            } else {
                info!("No se encontraron oportunidades no conflictivas para ejecutar.");
            }
        }
    }
}

fn calculate_dynamic_slippage(tvl: f64, net_profit_usd: f64) -> u32 {
    if tvl > 5_000_000.0 {
        if net_profit_usd < 100.0 { 8 } else if net_profit_usd < 1000.0 { 12 } else { 15 }
    } else if tvl > 500_000.0 {
        if net_profit_usd < 50.0 { 18 } else { 25 }
    } else { 40 }
}
