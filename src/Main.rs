/// Punto de entrada del binario.
/// Configura el runtime asíncrono de Tokio y lanza la función principal del bot.
#[tokio::main]
async fn main() {
    // Llama a la función `run` de nuestra librería (lib.rs).
    // Si `run` devuelve un error (lo que significa que el bot se detuvo por un fallo crítico),
    // lo imprimirá en el log y terminará el programa con un código de salida de error.
    if let Err(e) = mev_bot_arbitrage_v4::run().await {
        log::error!("La aplicación ha terminado con un error fatal: {:?}", e);
        std::process::exit(1);
    }
}
