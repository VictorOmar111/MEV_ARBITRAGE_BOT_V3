pub mod config;
pub mod constants;
pub mod execution;
pub mod multi;
pub mod oracle;
pub mod optimization;
pub mod paths;
pub mod pools;
pub mod provider;
pub mod simulator;
pub mod streams;
pub mod strategy;
pub mod types;
pub mod utils;

use crate::config::CONFIG;
use anyhow::Result;
use ethers::prelude::*;
use lazy_static::lazy_static;
use log::{error, info};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::task::JoinSet;

lazy_static! {
    static ref EXECUTED_OPPORTUNITIES: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
}

pub fn lock_opportunity(block_number: u64, path: &paths::ArbPath) -> bool {
    let key = path.key();
    let lock_key = format!("{}-{}", block_number, key);
    let mut executed = EXECUTED_OPPORTUNITIES.lock().unwrap();
    executed.insert(lock_key)
}

pub fn clear_old_locks(current_block: u64) {
    let mut executed = EXECUTED_OPPORTUNITIES.lock().unwrap();
    executed.retain(|k| {
        if let Some(bn_str) = k.split('-').next() {
            if let Ok(bn) = bn_str.parse::<u64>() {
                return bn.saturating_add(10) >= current_block;
            }
        }
        true
    });
}

pub async fn run() -> Result<()> {
    dotenv::dotenv().ok();
    utils::setup_logger()?;

    info!(" Arrancando MEV Harvester v4.0...");

    // --- FASE 1: Conexión e Inicialización ---
    let provider = Provider::<Http>::try_from(CONFIG.https_url.as_str())?;
    let wallet = CONFIG.private_key.parse::<LocalWallet>()?.with_chain_id(CONFIG.chain_id);
    let client = Arc::new(SignerMiddleware::new(provider, wallet));
    let provider_ws = Arc::new(Provider::<Ws>::connect(&CONFIG.wss_url).await?);
    let oracle_map = Arc::new(oracle::OracleMap::new());

    // --- FASE 2: Sincronización Inicial ---
    info!("Realizando sincronización inicial de pools (puede tardar varios minutos)...");
    let initial_pools = pools::load_all_pools_v3(provider_ws.clone(), &oracle_map).await?;
    let initial_paths = paths::generate_triangular_paths(&initial_pools, CONFIG.token_in_address, &oracle_map);

    // --- FASE 3: Lanzamiento de Tareas Asíncronas ---
    let (event_sender, _) = tokio::sync::broadcast::channel(512);
    let mut set = JoinSet::new();

    info!(" Lanzando tareas asíncronas...");
    set.spawn(streams::stream_new_blocks(provider_ws.clone(), event_sender.clone()));

    let strategy_client = client.clone();
    let strategy_oracles = oracle_map.clone();
    set.spawn(async move {
        if let Err(e) = strategy::event_handler(
            strategy_client,
            provider_ws,
            strategy_oracles,
            event_sender,
            initial_pools,
            initial_paths,
        )
        .await
        {
            error!("El manejador de estrategia ha fallado críticamente: {e:?}");
        }
    });

    info!(" Bot V4 corriendo. Presiona Ctrl+C para terminar.");

    // --- FASE 4: Gestión del Ciclo de Vida ---
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Señal de Ctrl+C recibida. Abortando todas las tareas...");
            set.abort_all();
            info!("Tareas abortadas. Saliendo.");
        }
        Some(res) = set.join_next() => {
            match res {
                Ok(_) => error!("Una tarea esencial ha terminado inesperadamente sin error."),
                Err(e) => error!("Una tarea esencial ha fallado (JoinError): {e:?}. El bot se detendrá."),
            }
        }
    }

    Ok(())
}
