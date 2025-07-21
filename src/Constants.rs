use ethers::types::H160;
use lazy_static::lazy_static;
use std::str::FromStr;

// Usamos `lazy_static` para parsear las direcciones desde string una sola vez.
lazy_static! {
    // --- Direcciones de Tokens Comunes (Arbitrum) ---
    pub static ref WETH_ADDRESS: H160 = H160::from_str("0x82af49447d8a07e3bd95bd0d56f35241523fbab1").unwrap();
    pub static ref USDC_ADDRESS: H160 = H160::from_str("0xaf88d065e77c8cC2239327C5EDb3A432268e5831").unwrap();
    pub static ref WBTC_ADDRESS: H160 = H160::from_str("0x2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f").unwrap();

    // --- Direcciones de Factories V3 (Arbitrum) ---
    pub static ref UNISWAP_V3_FACTORY: H160 = H160::from_str("0x1F98431c8aD98523631AE4a59f267346ea31F984").unwrap();
    pub static ref SUSHISWAP_V3_FACTORY: H160 = H160::from_str("0xbACEB8eC6b9355Dfc0269C18bac9d6E2Bdc29C4F").unwrap();
    pub static ref PANCAKESWAP_V3_FACTORY: H160 = H160::from_str("0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865").unwrap();

    // --- Direcciones de Quoters V2 (para simulación de swaps) ---
    pub static ref UNISWAP_V3_QUOTER: H160 = H160::from_str("0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6").unwrap();
    pub static ref SUSHISWAP_V3_QUOTER: H160 = H160::from_str("0xf2614A233c7C3e7f08b1F887Ba133a13f1eb2c55").unwrap();
    pub static ref PANCAKESWAP_V3_QUOTER: H160 = H160::from_str("0xFE6508f0015C778Bdcc1fB5465bA5ebE224C9912").unwrap();

    // --- Direcciones de Contratos de Oráculos (Arbitrum) ---
    // Contrato principal de Pyth Network
// Contrato principal de Pyth Network
    pub static ref PYTH_ORACLE_CONTRACT: H160 = H160::from_str("0xff1f2b4adb936f69af13e454ec231792e8dc5028").unwrap();
}

// --- Parámetros por Defecto para `config.rs` ---
pub const DEFAULT_GAS_LIMIT: u64 = 2_000_000;
pub const DEFAULT_MIN_PROFIT_USD: f64 = 0.1;
pub const DEFAULT_MIN_ORACLE_LAG: f64 = 0.08;
pub const DEFAULT_MAX_ORACLE_AGE_SECS: u64 = 120;
pub const DEFAULT_PATH_REFRESH_INTERVAL_BLOCKS: u64 = 100;
pub const DEFAULT_MAX_BRIBE_PERCENT: f64 = 0.80; // 80%
