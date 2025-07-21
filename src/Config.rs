use crate::constants;
use ethers::types::H160;
use once_cell::sync::Lazy;
use std::env;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
    // --- Conexión a la Red ---
    pub wss_url: String,
    pub https_url: String,
    pub chain_id: u64,

    // --- Wallet y Contratos ---
    pub private_key: String,
    pub contract_address: H160,
    pub balancer_vault: H160,

    // --- Estrategia de Arbitraje ---
    pub token_in_address: H160,
    pub min_profit_usd: f64,
    pub gas_limit: u64,

    // --- Parámetros de Agresividad y Sensibilidad ---
    pub min_oracle_lag: f64,
    pub max_oracle_age_secs: u64,
    pub path_refresh_interval_blocks: u64,
    pub max_bribe_percent: f64,

    // --- Operación General ---
    pub cache_path: String,
    pub cache_ttl_secs: u64,
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    // Carga las variables desde el archivo .env en la raíz del proyecto.
    dotenv::dotenv().ok();

    Config {
        // --- Conexión (Críticas, el programa fallará si no están) ---
        wss_url: env::var("WSS_URL").expect("Falta WSS_URL en .env"),
        https_url: env::var("HTTPS_URL").expect("Falta HTTPS_URL en .env"),
        chain_id: env::var("CHAIN_ID")
            .expect("Falta CHAIN_ID en .env")
            .parse()
            .expect("CHAIN_ID inválido, debe ser un número"),

        // --- Wallet y Contratos (Críticas) ---
        private_key: env::var("PRIVATE_KEY").expect("Falta PRIVATE_KEY en .env"),
        contract_address: H160::from_str(
            &env::var("CONTRACT_ADDRESS").expect("Falta CONTRACT_ADDRESS en .env"),
        )
        .expect("CONTRACT_ADDRESS inválido"),
        balancer_vault: H160::from_str(
            &env::var("BALANCER_VAULT").expect("Falta BALANCER_VAULT en .env"),
        )
        .expect("BALANCER_VAULT inválido"),

        // --- Estrategia (Crítica la principal, las demás tienen defaults) ---
        token_in_address: H160::from_str(
            &env::var("TOKEN_IN_ADDRESS").expect("Falta TOKEN_IN_ADDRESS en .env"),
        )
        .expect("TOKEN_IN_ADDRESS inválido"),

        // --- Parámetros con valores por defecto del archivo `constants.rs` ---
        min_profit_usd: env::var("MIN_PROFIT_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(constants::DEFAULT_MIN_PROFIT_USD),
        gas_limit: env::var("GAS_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(constants::DEFAULT_GAS_LIMIT),
        min_oracle_lag: env::var("MIN_ORACLE_LAG")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(constants::DEFAULT_MIN_ORACLE_LAG),
        max_oracle_age_secs: env::var("MAX_ORACLE_AGE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(constants::DEFAULT_MAX_ORACLE_AGE_SECS),
        path_refresh_interval_blocks: env::var("PATH_REFRESH_INTERVAL_BLOCKS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(constants::DEFAULT_PATH_REFRESH_INTERVAL_BLOCKS),
        max_bribe_percent: env::var("MAX_BRIBE_PERCENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(constants::DEFAULT_MAX_BRIBE_PERCENT),

        // --- Operación ---
        cache_path: env::var("CACHE_PATH")
            .unwrap_or_else(|_| "cache/pools_v4.csv".to_string()),
        cache_ttl_secs: env::var("CACHE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(86400), // 24 horas
    }
});
