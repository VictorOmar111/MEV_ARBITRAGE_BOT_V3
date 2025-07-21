use ethers::{
    prelude::*,
    providers::{Middleware, Provider, Ws},
};
use futures_util::StreamExt;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::broadcast::Sender;

/// Define los eventos que el bot puede procesar.
/// Por ahora, el principal es `Block`, que actúa como el "latido" del bot.
#[derive(Clone, Debug)]
pub enum Event {
    Block(Block<H256>),
    MempoolTx(Transaction),
}

/// Escucha el stream de nuevos bloques de la red y emite un evento `Event::Block`
/// para cada uno. Este es el disparador principal de nuestra estrategia.
pub async fn stream_new_blocks(provider: Arc<Provider<Ws>>, sender: Sender<Event>) {
    let mut stream = match provider.subscribe_blocks().await {
        Ok(s) => s,
        Err(e) => {
            error!(" No se pudo suscribir a los bloques: {e:?}");
            return;
        }
    };
    info!(" Subscripción a nuevos bloques iniciada.");

    while let Some(block_header) = stream.next().await {
        if let Some(hash) = block_header.hash {
            // Obtenemos el bloque completo, ya que contiene información valiosa como el `base_fee_per_gas`.
            match provider.get_block(hash).await {
                Ok(Some(full_block)) => {
                    if sender.send(Event::Block(full_block)).is_err() {
                        // Esto ocurre si el receptor (el `strategy_handler`) ha terminado.
                        // Podemos salir del bucle para no seguir trabajando inútilmente.
                        warn!("El canal de eventos de bloques está cerrado. Terminando stream.");
                        break;
                    }
                }
                Ok(None) => warn!("Reorganización de bloque detectada, el bloque {hash:?} ya no existe."),
                Err(e) => error!("Error al obtener el bloque completo {hash:?}: {e:?}"),
            }
        }
    }
}

/// (Opcional) Escucha el mempool para transacciones pendientes.
/// Útil para estrategias de back-running. Puede ser intensivo en recursos.
pub async fn stream_pending_txs(provider: Arc<Provider<Ws>>, sender: Sender<Event>) {
    let mut stream = match provider.subscribe_pending_txs().await {
        Ok(s) => s,
        Err(e) => {
            error!(" No se pudo suscribir al mempool: {e:?}");
            return;
        }
    };
    info!(" Subscripción al mempool iniciada.");

    while let Some(tx_hash) = stream.next().await {
        let provider_clone = provider.clone();
        let sender_clone = sender.clone();
        // Lanzamos una tarea separada para obtener los detalles de la TX.
        // Esto evita que una llamada lenta a `get_transaction` bloquee todo el stream.
        tokio::spawn(async move {
            if let Ok(Some(tx)) = provider_clone.get_transaction(tx_hash).await {
                if sender_clone.send(Event::MempoolTx(tx)).is_err() {
                    // No logueamos como error, ya que el consumidor puede estar ocupado o cerrado.
                }
            }
        });
    }
}
