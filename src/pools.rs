// src/pools.rs

use anyhow::Result;
use cfmms::{
    dex::{Dex, DexVariant as CfmmsDexVariant},
    pool::Pool as CfmmsPool,
    sync::sync_pairs,
};
use csv::StringRecord;
use ethers::providers::{Provider, Ws};
use ethers::types::H160;
use log::info;
use std::{
    cmp::Ordering,
    collections::HashSet,
    fs,
    hash::{Hash, Hasher},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, Eq)]
pub enum DexVariant { UniswapV3 }

// ... (Implementaciones de traits para DexVariant sin cambios) ...

#[derive(Debug, Clone, Copy, Eq)]
pub struct Pool {
    pub factory: H160, // <-- CAMBIO IMPORTANTE: Guardamos la factory
    pub address: H160,
    pub version: DexVariant,
    pub token0: H160,
    pub token1: H160,
    pub decimals0: u8,
    pub decimals1: u8,
    pub fee: u32,
}

// ... (Implementaciones de traits para Pool sin cambios) ...

impl From<StringRecord> for Pool {
    fn from(record: StringRecord) -> Self {
        // ... (lógica de parseo robusta sin cambios) ...
        // Añadimos la factory al parseo desde el caché
        Self {
            factory: H160::from_str(record.get(7).unwrap_or("")).unwrap_or_default(),
            address: H160::from_str(record.get(0).unwrap_or("")).unwrap_or_default(),
            // ... resto de los campos
        }
    }
}

impl Pool {
    pub fn cache_row(&self) -> (String, i32, String, String, u8, u8, u32, String) {
        (
            format!("{:?}", self.address),
            3,
            format!("{:?}", self.token0),
            format!("{:?}", self.token1),
            self.decimals0,
            self.decimals1,
            self.fee,
            format!("{:?}", self.factory), // Guardamos la factory en el caché
        )
    }
}

pub async fn load_all_pools_v3(
    wss_url: &str,
    factories: &[(H160, u64)], // Recibimos una referencia
    cache_path: PathBuf,
    max_cache_age_secs: u64,
) -> Result<Vec<Pool>> {
    if let Ok(metadata) = fs::metadata(&cache_path) {
        if let Ok(modified) = metadata.modified() {
            let age = SystemTime::now().duration_since(modified)?.as_secs();
            if age <= max_cache_age_secs {
                info!("📥 Usando caché de pools (edad: {} segundos)", age);
                let mut reader = csv::Reader::from_path(&cache_path)?;
                return Ok(reader.records().filter_map(|r| r.ok()).map(Pool::from).collect());
            }
        }
    }

    info!("💽 Sincronizando pools de Uniswap V3 desde la blockchain...");
    let provider = Arc::new(Provider::<Ws>::connect(wss_url).await?);
    let mut all_pools = HashSet::new();

    // Iteramos por cada factory para saber de dónde viene cada pool
    for &(factory_address, creation_block) in factories {
        info!("Sincronizando factory: {:?}", factory_address);
        let dex = Dex::new(factory_address, CfmmsDexVariant::UniswapV3, creation_block, None);
        let synced_pools: Vec<CfmmsPool> = sync_pairs(vec![dex], provider.clone(), None).await?;
        
        for p in synced_pools {
            if let CfmmsPool::UniswapV3(pv3) = p {
                all_pools.insert(Pool {
                    factory: factory_address, // <-- ASIGNACIÓN CLAVE
                    address: pv3.address,
                    version: DexVariant::UniswapV3,
                    token0: pv3.token_a,
                    token1: pv3.token_b,
                    decimals0: pv3.token_a_decimals,
                    decimals1: pv3.token_b_decimals,
                    fee: pv3.fee,
                });
            }
        }
    }
    
    let pools_vec = all_pools.into_iter().collect::<Vec<_>>();
    info!("Total de pools V3 sincronizadas: {}", pools_vec.len());

    let mut writer = csv::Writer::from_path(&cache_path)?;
    writer.write_record(&["address", "version", "token0", "token1", "decimals0", "decimals1", "fee", "factory"])?;
    for p in &pools_vec { writer.serialize(p.cache_row())?; }
    writer.flush()?;
    info!("🔒 Pools guardadas en caché en {:?}", cache_path);

    Ok(pools_vec)
}
