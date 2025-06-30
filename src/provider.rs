// src/provider.rs

use ethers::prelude::*;
use std::str::FromStr;
use std::sync::Arc;

use crate::config::CONFIG;

abigen!(
    AggregatorV3Interface,
    r#"[
        function latestRoundData() external view returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound)
        function decimals() external view returns (uint8)
    ]"#
);

pub async fn get_eth_price() -> f64 {
    let provider = Provider::<Http>::try_from(CONFIG.https_url.clone()).expect("Failed to connect to provider");
    let client = Arc::new(provider);
    
    // Dirección del Price Feed de ETH/USD de Chainlink en Mainnet
    let feed_address: H160 = "0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419".parse().unwrap();
    let contract = AggregatorV3Interface::new(feed_address, client);

    if let Ok((_, price, _, _, _)) = contract.latest_round_data().call().await {
        let decimals = contract.decimals().call().await.unwrap_or(8) as u32;
        let price_dec = (price.as_u128() as f64) / 10f64.powi(decimals as i32);
        return price_dec;
    }
    
    // Fallback por si Chainlink falla
    0.0
}
