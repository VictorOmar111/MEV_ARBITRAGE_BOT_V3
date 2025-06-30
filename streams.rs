// src/streams.rs

use anyhow::Result;
use ethers::{
    providers::{Middleware, Provider, Ws},
    types::{Filter, Log, Transaction, U256, U64},
};
use tokio_stream::StreamExt;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::time::{sleep, Duration};

use crate::utils::calculate_next_block_base_fee;

#[derive(Clone, Debug)]
pub enum Event {
    Block(NewBlock),
    PendingTx(Transaction),
    Log(Log),
}

#[derive(Clone, Debug, Default)]
pub struct NewBlock {
    pub block_number: U64,
    pub base_fee: U256,
    pub next_base_fee: U256,
}

/// Escucha nuevos bloques de forma persistente con lógica de auto-reconexión.
pub async fn stream_new_blocks(provider: Arc<Provider<Ws>>, event_sender: Sender<Event>) -> Result<()> {
    loop {
        info!("[STREAM] Conectando al stream de nuevos bloques...");
        
        match provider.subscribe_blocks().await {
            Ok(stream) => {
                info!("[STREAM] Conexión exitosa al stream de bloques. Escuchando...");
                let mut stream = stream.filter_map(|block| match block.number {
                    Some(number) => Some(NewBlock {
                        block_number: number,
                        base_fee: block.base_fee_per_gas.unwrap_or_default(),
                        next_base_fee: calculate_next_block_base_fee(
                            block.gas_used,
                            block.gas_limit,
                            block.base_fee_per_gas.unwrap_or_default(),
                        ),
                    }),
                    None => None,
                });

                while let Some(block) = stream.next().await {
                    if let Err(e) = event_sender.send(Event::Block(block)) {
                        error!("[STREAM] Error enviando evento de nuevo bloque al canal: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("[STREAM] No se pudo suscribir al stream de bloques: {:?}", e);
            }
        }
        
        warn!("[STREAM] El stream de bloques ha terminado. Reconectando en 5 segundos...");
        sleep(Duration::from_secs(5)).await;
    }
}
