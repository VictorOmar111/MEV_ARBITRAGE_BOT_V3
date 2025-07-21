use crate::config::CONFIG;
use anyhow::{Result, Error};
use ethers::{
    prelude::*,
    providers::{Http, Provider},
};
use std::{sync::Arc, time::Duration};

/// Establece la conexión principal con el proveedor RPC (HTTP).
/// Esta conexión se usará para todas las consultas on-chain y el envío de transacciones.
pub fn connect_provider() -> Result<Arc<Provider<Http>>> {
    // Intenta crear un proveedor desde la URL en la configuración.
    // El `.interval()` establece la frecuencia con la que `ethers-rs` consulta al nodo,
    // lo que ayuda a evitar ser rate-limited. 500ms es un valor razonable.
    let provider = Provider::<Http>::try_from(CONFIG.https_url.as_str())?
        .interval(Duration::from_millis(500));

    // Envolvemos el proveedor en un Arc (Atomic Reference Counting) para poder
    // compartirlo de forma segura y eficiente entre todas las tareas asíncronas del bot.
    Ok(Arc::new(provider))
}

/// Estima el gas para una llamada de contrato con una lógica de reintentos y un margen de seguridad.
pub async fn estimate_gas<M: Middleware>(
    call: &ContractCall<M, ()>,
) -> Result<U256> {
    // Intenta estimar el gas hasta 3 veces con un pequeño delay entre intentos.
    for attempt in 0..3 {
        if let Ok(gas) = call.estimate_gas().await {
            // Si la estimación tiene éxito, le añadimos un buffer del 25% por seguridad.
            // Esto ayuda a prevenir que la transacción falle por cambios mínimos en el estado.
            return Ok(gas * 125 / 100);
        }
        tokio::time::sleep(Duration::from_millis(50 * (attempt + 1))).await;
    }

    // Si después de 3 intentos la estimación falla, usamos el valor de fallback
    // definido en nuestra configuración. Es un valor alto para asegurar la ejecución.
    Ok(U256::from(CONFIG.gas_limit))
}
