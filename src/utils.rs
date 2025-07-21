use anyhow::Result;
use chrono::Local;
use fern::colors::{Color, ColoredLevelConfig};
use log::LevelFilter;

/// Configura el logger global para la aplicación.
/// Esto nos permite ver los logs (info, warn, error) en la consola de una manera
/// legible y con colores para diferenciar la severidad de los mensajes.
pub fn setup_logger() -> Result<()> {
    // Configuración de colores para los diferentes niveles de log.
    let colors = ColoredLevelConfig::new()
        .info(Color::Green)
        .warn(Color::Yellow)
        .error(Color::Red)
        .debug(Color::White);

    // Creación y aplicación del despachador de logs.
    fern::Dispatch::new()
        // Formato de cada línea de log. Incluye timestamp, nivel coloreado y el mensaje.
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                Local::now().format("[%H:%M:%S]"), // Timestamp ej: [14:35:10]
                colors.color(record.level()),      // Nivel ej: [INFO]
                message                            // El mensaje del log
            ))
        })
        // Nivel de log por defecto para nuestro bot. Veremos INFO y superiores.
        .level(LevelFilter::Info)
        // Reducimos el ruido de las librerías externas como ethers y hyper.
        .level_for("ethers", LevelFilter::Warn)
        .level_for("hyper", LevelFilter::Warn)
        // Enviamos el output a la salida estándar (la consola).
        .chain(std::io::stdout())
        // Aplicamos la configuración.
        .apply()?;

    Ok(())
}
