// src/main.rs

pub mod config;
pub mod constants;
pub mod execution;
pub mod math;
pub mod multi;
pub mod paths;
pub mod pools;
pub mod provider;
pub mod simulator;
pub mod streams;
pub mod strategy;
pub mod utils;

use anyhow::Result;
use ethers::providers::{Provider, Ws};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::broadcast::{self, Sender};
use tokio::task::JoinSet;
use tokio::signal;

use crate::constants::Env;
use crate::strategy::event_handler;
use crate::streams::{stream_new_blocks, Event};
use crate::utils::setup_logger;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    setup_logger()?;

    info!("Iniciando bot de arbitraje V2...");
    let env = Env::new();

    let ws = Ws::connect(env.wss_url.clone()).await?;
    let provider_ws = Arc::new(Provider::new(ws));
    let (event_sender, _): (Sender<Event>, _) = broadcast::channel(512);

    let mut set = JoinSet::new();

    set.spawn(streams::stream_new_blocks(
        provider_ws.clone(),
        event_sender.clone(),
    ));
    
    set.spawn(async move {
        if let Err(e) = event_handler(provider_ws.clone(), event_sender.clone()).await {
            log::error!("El manejador de estrategia ha fallado: {:?}", e);
        }
    });

    set.spawn(async {
        signal::ctrl_c().await.expect("Fallo escuchando Ctrl+C");
        info!("Señal de Ctrl+C recibida, cerrando bot...");
    });

    info!("Bot V2 corriendo. Esperando nuevos bloques...");

    while let Some(res) = set.join_next().await {
        match res {
            Ok(_) => info!("Una tarea ha finalizado su ciclo sin pánico."),
            Err(e) => error!("Una tarea ha fallado o ha sido cancelada (JoinError): {:?}", e),
        }
    }

    Ok(())
}
