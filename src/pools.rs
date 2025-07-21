use crate::{
    config::CONFIG,
    constants::USDC_ADDRESS,
    multi::batch_get_pool_data,
    oracle::OracleMap,
    types::{DexVariant, Pool},
};
use anyhow::{anyhow, Result};
use ethers::{prelude::*, types::H160};
use log::{info, warn};
use rust_decimal::{prelude::FromPrimitive, prelude::ToPrimitive, Decimal};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    path::PathBuf,
    sync::Arc,
    time::SystemTime,
    str::FromStr,
};

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct GraphData { p100: Vec<GraphPool>, p500: Vec<GraphPool>, p3000: Vec<GraphPool>, p10000: Vec<GraphPool> }
#[derive(Deserialize, Debug, Clone)]
struct GraphPool { id: H160, #[serde(rename = "feeTier")] fee_tier: String, token0: GraphToken, token1: GraphToken }
#[derive(Deserialize, Debug, Clone)]
struct GraphToken { id: H160, decimals: String }
#[derive(Deserialize, Debug)]
struct GraphResponse { data: Option<GraphData> }

/// Carga los pools directamente desde el archivo de caché y los enriquece con datos en tiempo real.
pub async fn load_all_pools_v3(
    provider: Arc<Provider<Ws>>,
    oracle_map: &Arc<OracleMap>,
) -> Result<Vec<Pool>> {
    let cache_path = PathBuf::from(&CONFIG.cache_path);
    info!(" Cargando mapa de pools pre-descubiertos desde {:?}...", cache_path);

    let file = File::open(&cache_path)
        .map_err(|_| anyhow!("FATAL: No se encontró el archivo de caché 'cache/pools_v4.csv'. Por favor, créalo primero con el script de Python."))?;

    let mut rdr = csv::Reader::from_reader(file);
    let mut pools: Vec<Pool> = rdr.deserialize().filter_map(Result::ok).collect();

    if pools.is_empty() {
        return Err(anyhow!("FATAL: La caché de pools está vacía. El bot no puede operar."));
    }

    info!("Cargados {} pools desde la caché. Enriqueciendo con datos en tiempo real...", pools.len());

    let pool_addresses: Vec<H160> = pools.iter().map(|p| p.address).collect();
    let raw_data = batch_get_pool_data(provider.clone(), &pool_addresses).await?;
    info!("Datos en lote (liquidez/balances) obtenidos para {} pools.", raw_data.len());

    let mut unique_tokens = HashSet::new();
    for data in raw_data.values() {
        unique_tokens.insert(data.token0);
        unique_tokens.insert(data.token1);
    }

    let mut price_map = HashMap::new();
    let known_tokens = [ *USDC_ADDRESS, crate::constants::WETH_ADDRESS.clone() ];
    for &token in &known_tokens {
        if let Some(price_info) = oracle_map.get_price::<Provider<Ws>>(&token, provider.clone()).await {
            price_map.insert(token, price_info.price);
        }
    }
    price_map.insert(*USDC_ADDRESS, 1.0);

    for data in raw_data.values() {
        let (t0, t1) = (data.token0, data.token1);
        let (p0_known, p1_known) = (price_map.contains_key(&t0), price_map.contains_key(&t1));

        if p0_known && !p1_known {
            if let Ok(sqrt_price_x96) = Decimal::from_str(&data.sqrt_price_x96.to_string()) {
                let price0 = *price_map.get(&t0).unwrap();
                let price_t1_t0 = (sqrt_price_x96 / Decimal::from_u128(2u128.pow(96)).unwrap()).powi(2);
                let price_t0_t1 = Decimal::ONE / price_t1_t0;
                let price1 = price0 * (price_t0_t1.to_f64().unwrap_or(0.0) * 10f64.powi((data.decimals0 as i32) - (data.decimals1 as i32)));
                if price1 > 0.0 { price_map.insert(t1, price1); }
            }
        } else if !p0_known && p1_known {
            if let Ok(sqrt_price_x96) = Decimal::from_str(&data.sqrt_price_x96.to_string()) {
                let price1 = *price_map.get(&t1).unwrap();
                let price_t1_t0 = (sqrt_price_x96 / Decimal::from_u128(2u128.pow(96)).unwrap()).powi(2);
                let price0 = price1 * (price_t1_t0.to_f64().unwrap_or(0.0) * 10f64.powi((data.decimals1 as i32) - (data.decimals0 as i32)));
                if price0 > 0.0 { price_map.insert(t0, price0); }
            }
        }
    }
    info!("Mapa de precios expandido a {} tokens por derivación.", price_map.len());

    for pool in &mut pools {
        if let Some(data) = raw_data.get(&pool.address) {
            let price0 = price_map.get(&data.token0).cloned().unwrap_or(0.0);
            let price1 = price_map.get(&data.token1).cloned().unwrap_or(0.0);
            if price0 == 0.0 || price1 == 0.0 { pool.tvl_usd = 0.0; continue; }

            let balance0_dec = Decimal::from_u128(data.balance0.as_u128()).unwrap_or_default() / Decimal::from(10u128.pow(data.decimals0 as u32));
            let balance1_dec = Decimal::from_u128(data.balance1.as_u128()).unwrap_or_default() / Decimal::from(10u128.pow(data.decimals1 as u32));
            pool.tvl_usd = (Decimal::from_f64(price0).unwrap_or_default() * balance0_dec + Decimal::from_f64(price1).unwrap_or_default() * balance1_dec).to_f64().unwrap_or(0.0);
        }
    }

    let final_pools: Vec<Pool> = pools.into_iter().filter(|p| p.tvl_usd > 10_000_000.0).collect();
    info!("Total de pools con TVL > $10M listos para operar: {}", final_pools.len());

    Ok(final_pools)
}
