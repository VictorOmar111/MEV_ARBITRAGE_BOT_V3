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
use log::{info, warn};
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

/// Enum para identificar la versión del DEX. Ahora solo V3.
#[derive(Debug, Clone, Copy, Eq)]
pub enum DexVariant { UniswapV3 }

// Implementaciones de traits para que DexVariant pueda ser usado en colecciones.
impl PartialEq for DexVariant { /* ... sin cambios ... */ }
impl PartialOrd for DexVariant { /* ... sin cambios ... */ }
impl Ord for DexVariant { /* ... sin cambios ... */ }
impl Hash for DexVariant { fn hash<H: Hasher>(&self, s: &mut H) { core::mem::discriminant(self).hash(s) } }

/// Struct local para representar un pool con la información esencial.
#[derive(Debug, Clone, Copy, Eq)]
pub struct Pool {
    pub address: H160,
    pub version: DexVariant,
    pub token0: H160,
    pub token1: H160,
    pub decimals0: u8,
    pub decimals1: u8,
    pub fee: u32, // Para V3, esto es 100, 500, o 3000
}

// Implementaciones de traits para que Pool pueda ser usado en colecciones como HashSet.
impl PartialEq for Pool { fn eq(&self, other: &Self) -> bool { self.address == other.address } }
impl PartialOrd for Pool { fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) } }
impl Ord for Pool { fn cmp(&self, other: &Self) -> Ordering { self.address.cmp(&other.address) } }
impl Hash for Pool { fn hash<H: Hasher>(&self, h: &mut H) { self.address.hash(h) } }

/// Convierte una fila de un archivo CSV a nuestro struct Pool.
impl From<StringRecord> for Pool {
    fn from(record: StringRecord) -> Self {
        // Ahora solo parseamos V3, pero mantenemos la lógica por si se reutiliza.
        let version = match record.get(1).unwrap_or("3") {
            "3" => DexVariant::UniswapV3,
            _ => {
                warn!("Se encontró un tipo de pool no esperado en el caché, asumiendo V3");
                DexVariant::UniswapV3
            }
        };

        Self {
            address: H160::from_str(record.get(0).unwrap_or("")).unwrap_or_default(),
            version,
            token0: H160::from_str(record.get(2).unwrap_or("")).unwrap_or_default(),
            token1: H160::from_str(record.get(3).unwrap_or("")).unwrap_or_default(),
            decimals0: record.get(4).unwrap_or("18").parse().unwrap_or(18),
            decimals1: record.get(5).unwrap_or("18").parse().unwrap_or(18),
            fee: record.get(6).unwrap_or("500").parse().unwrap_or(500),
        }
    }
}

impl Pool {
    /// Genera una tupla para ser guardada como una fila en el archivo CSV.
    pub fn cache_row(&self) -> (String, i32, String, String, u8, u8, u32) {
        (
            format!("{:?}", self.address),
            3, // Hardcodeado a 3 ya que solo usamos V3
            format!("{:?}", self.token0),
            format!("{:?}", self.token1),
            self.decimals0,
            self.decimals1,
            self.fee,
        )
    }
}

/// Sincroniza todos los pools de Uniswap V3 desde las fábricas proporcionadas.
/// Usa un archivo caché para acelerar los inicios posteriores.
pub async fn load_all_pools_v3(
    wss_url: &str,
    factories: Vec<(H160, u64)>,
    cache_path: PathBuf,
    max_cache_age_secs: u64,
    checkpoint: Option<&str>, // Para reanudar la sincronización
) -> Result<Vec<Pool>> {
    
    if let Ok(metadata) = fs::metadata(&cache_path) {
        if let Ok(modified) = metadata.modified() {
            let age = SystemTime::now().duration_since(modified)?.as_secs();
            if age <= max_cache_age_secs {
                info!("📥 Usando caché de pools (edad: {} segundos)", age);
                let mut reader = csv::Reader::from_path(&cache_path)?;
                let pools = reader.records().filter_map(|r| r.ok()).map(Pool::from).collect();
                return Ok(pools);
            }
        }
    }

    info!("💽 Sincronizando pools de Uniswap V3 desde la blockchain...");
    let provider = Arc::new(Provider::<Ws>::connect(wss_url).await?);

    let dexes_to_sync: Vec<Dex> = factories.into_iter()
        .map(|(address, creation_block)| Dex::new(address, CfmmsDexVariant::UniswapV3, creation_block, None))
        .collect();

    let synced_pools: Vec<CfmmsPool> = sync_pairs(dexes_to_sync, provider.clone(), checkpoint.map(String::from)).await?;

    // Usamos un HashSet para eliminar duplicados de forma eficiente
    let pools_set: HashSet<Pool> = synced_pools.into_iter()
        .filter_map(|p| match p {
            CfmmsPool::UniswapV3(pv3) => Some(Pool {
                address: pv3.address,
                version: DexVariant::UniswapV3,
                token0: pv3.token_a,
                token1: pv3.token_b,
                decimals0: pv3.token_a_decimals,
                decimals1: pv3.token_b_decimals,
                fee: pv3.fee,
            }),
            _ => None, // Ignoramos cualquier otro tipo de pool
        })
        .collect();
    
    let pools = pools_set.into_iter().collect::<Vec<_>>();
    info!("Total de pools V3 sincronizadas: {}", pools.len());

    let mut writer = csv::Writer::from_path(&cache_path)?;
    writer.write_record(&["address", "version", "token0", "token1", "decimals0", "decimals1", "fee"])?;
    for p in &pools { writer.serialize(p.cache_row())?; }
    writer.flush()?;
    info!("🔒 Pools guardadas en caché en {:?}", cache_path);

    Ok(pools)
}
