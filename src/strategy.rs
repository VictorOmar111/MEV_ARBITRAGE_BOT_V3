// src/strategy.rs

use ethers::{prelude::*, types::{H160, U256}};
use log::{info, warn};
use std::{collections::HashMap, str::FromStr, sync::Arc, path::PathBuf};
use tokio::sync::broadcast::Sender;

use crate::{
    config::CONFIG,
    constants::Env,
    execution::execute_flashloan_transaction,
    math::{calculate_net_profit, calculate_optimal_input},
    paths::{generate_triangular_paths, ArbPath},
    pools::{load_all_pools_v3, Pool},
    streams::Event,
    utils::get_touched_pool_reserves,
};

#[derive(Clone, Debug)]
pub struct DexConfig {
    pub name: &'static str,
    pub router_key_str: &'static str,
    pub factory_address: H160,
    pub factory_creation_block: u64,
}

#[derive(Clone, Debug)]
pub struct ArbitrageOpportunity {
    pub path: ArbPath,
    pub dex_config: DexConfig,
    pub optimal_amount_in: U256,
    pub net_profit_usd: f64,
    pub expected_output: U256,
}

pub async fn event_handler(
    provider_ws: Arc<Provider<Ws>>,
    event_sender: Sender<Event>,
) -> anyhow::Result<()> {
    let env = Env::new();
    let http_provider = Arc::new(Provider::<Http>::try_from(env.https_url.clone())?);

    let dex_configs = vec![
        DexConfig {
            name: "Uniswap V3",
            router_key_str: "UniswapV3", // Debe coincidir con tu contrato
            factory_address: H160::from_str("0x1F98431c8aD98523631AE4a59f267346ea31F984")?,
            factory_creation_block: 420,
        },
        // Aquí puedes añadir otras fábricas V3 de Arbitrum
    ];

    info!("🤖 Monitoreando {} DEXs en Arbitrum.", dex_configs.len());
    let dex_factories: Vec<(H160, u64)> = dex_configs.iter().map(|d| (d.factory_address, d.creation_block)).collect();

    let pools = load_all_pools_v3(
        &env.wss_url,
        &dex_factories,
        PathBuf::from(&env.cache_path),
        env.cache_ttl_secs,
        env.checkpoint_path.as_deref(),
    ).await?;

    let token_in = H160::from_str(&env.token_in)?;
    let paths = generate_triangular_paths(&pools, token_in);
    info!("🏁 Rutas pre-generadas: {}", paths.len());

    let mut reserves: HashMap<H160, crate::multi::Reserve> = HashMap::new(); // Asumiendo que `multi` tiene el struct Reserve
    let mut event_receiver = event_sender.subscribe();

    info!("✅ Estrategia iniciada. Esperando nuevos bloques...");

    loop {
        if let Ok(Event::Block(block)) = event_receiver.recv().await {
            info!("🔔 Nuevo Bloque #{}", block.block_number);

            let touched = get_touched_pool_reserves(provider_ws.clone(), block.block_number).await.unwrap_or_default();
            if touched.is_empty() {
                info!("Sin actividad en pools relevantes.");
                continue;
            }
            reserves.extend(touched);

            let mut best_opportunity: Option<ArbitrageOpportunity> = None;

            for path in &paths {
                if !path.pools().iter().all(|p| reserves.contains_key(p)) { continue; }

                if let Some(optimal_amount) = calculate_optimal_input(path, &reserves) {
                    if optimal_amount.is_zero() { continue; }

                    let (net_profit, expected_out) = calculate_net_profit(http_provider.clone(), optimal_amount, path, &reserves).await;

                    if net_profit <= CONFIG.get()?.min_profit_usd { continue; }

                    if best_opportunity.as_ref().map_or(true, |best| net_profit > best.net_profit_usd) {
                        info!("💡 Oportunidad mejorada: profit=${:.2}", net_profit);
                        
                        // --- LÓGICA CORREGIDA: Encuentra el DexConfig correcto usando el campo `factory` del pool ---
                        let dex_config = dex_configs.iter()
                            .find(|d| d.factory_address == path.pool_1.factory)
                            .expect("Factory de la ruta no encontrada. Esto no debería pasar.")
                            .clone();

                        best_opportunity = Some(ArbitrageOpportunity {
                            path: path.clone(),
                            dex_config,
                            optimal_amount_in: optimal_amount,
                            net_profit_usd: net_profit,
                            expected_output: expected_out,
                        });
                    }
                }
            }
            
            if let Some(winner) = best_opportunity {
                info!(
                    "🚀 Ejecutando arbitraje en {}! Profit: ${:.2}, In: {}",
                    winner.dex_config.name, winner.net_profit_usd, winner.optimal_amount_in
                );
                if let Err(e) = execute_flashloan_transaction(&winner).await {
                    warn!("❗ Error en ejecución de la transacción: {:?}", e);
                }
            } else {
                info!("No se encontraron oportunidades rentables en este bloque.");
            }
        }
    }
}
