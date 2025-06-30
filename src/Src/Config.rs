// src/config.rs

use ethers::types::H160;
use once_cell::sync::Lazy;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub wss_url: String,
    pub https_url: String,
    pub chain_id: u64,
    pub private_key: String,
    pub contract_address: H160,
    pub bribe_percent: f64,
    pub min_profit_usd: f64,
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| Config {
    wss_url: env::var("WSS_URL").expect("WSS_URL must be set"),
    https_url: env::var("HTTPS_URL").expect("HTTPS_URL must be set"),
    chain_id: env::var("CHAIN_ID")
        .expect("CHAIN_ID must be set")
        .parse()
        .expect("CHAIN_ID must be a valid number"),
    private_key: env::var("PRIVATE_KEY").expect("PRIVATE_KEY must be set"),
    contract_address: env::var("CONTRACT_ADDRESS")
        .expect("CONTRACT_ADDRESS must be set")
        .parse()
        .expect("CONTRACT_ADDRESS must be a valid address"),
    bribe_percent: env::var("BRIBE_PERCENT")
        .expect("BRIBE_PERCENT must be set")
        .parse()
        .expect("BRIBE_PERCENT must be a valid float"),
    min_profit_usd: env::var("MIN_PROFIT_USD")
        .expect("MIN_PROFIT_USD must be set")
        .parse()
        .expect("MIN_PROFIT_USD must be a valid float"),
});
