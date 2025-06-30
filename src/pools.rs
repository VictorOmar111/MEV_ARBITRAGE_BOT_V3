// src/pools.rs

use anyhow::Result;
use cfmms::{
    dex::{Dex, DexVariant as CfmmsDexVariant},
    pool::Pool as CfmmsPool,
    sync::sync_pairs,
};
use csv::StringRecord;
use ethers::{
    providers::{Provider, Ws},
    types::H160,
};
use log::info;
use std::{cmp::Ordering, collections::BTreeSet, hash::{Hash, Hasher}, path::Path as FilePath, str::FromStr, sync::Arc};

#[derive(Debug, Clone, Copy, Eq)]
pub enum DexVariant {
    UniswapV2,
    UniswapV3,
}

// Implementaciones manuales para que BTreeSet funcione correctamente
impl PartialEq for DexVariant {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}
impl PartialOrd for DexVariant {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for DexVariant {
    fn cmp(&self, other: &Self) -> Ordering {
        (core::mem::discriminant(self) as u32).cmp(&(core::mem::discriminant(other) as u32))
    }
}
impl Hash for DexVariant {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
    }
}


#[derive(Debug, Clone, Copy, Eq)]
pub struct Pool {
    pub address: H160,
    pub version: DexVariant,
    pub token0: H160,
    pub token1: H160,
    pub decimals0: u8,
    pub decimals1: u8,
    pub fee: u32,
}

// Implementaciones manuales para que BTreeSet funcione correctamente
impl PartialEq for Pool {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address
    }
}
impl PartialOrd for Pool {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pool {
    fn cmp(&self, other: &Self) -> Ordering {
        self.address.cmp(&other.address)
    }
}
impl Hash for Pool {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.address.hash(state);
    }
}


impl From<StringRecord> for Pool {
    fn from(record: StringRecord) -> Self {
        let version = match record.get(1).unwrap_or("2") {
            "3" => DexVariant::UniswapV3,
            _ => DexVariant::UniswapV2,
        };

        Self {
            address: H160::from_str(record.get(0).unwrap_or("")).unwrap_or_default(),
            version,
            token0: H160::from_str(record.get(2).unwrap_or("")).unwrap_or_default(),
            token1: H160::from_str(record.get(3).unwrap_or("")).unwrap_or_default(),
            decimals0: record.get(4).unwrap_or("18").parse().unwrap_or(18),
            decimals1: record.get(5).unwrap_or("18").parse().unwrap_or(18),
            fee: record.get(6).unwrap_or("3000").parse().unwrap_or(3000),
        }
    }
}

impl Pool {
    pub fn cache_row(&self) -> (String, i32, String, String, u8, u8, u32) {
        (
            format!("{:?}", self.address),
            match self.version {
                DexVariant::UniswapV2 => 2,
                DexVariant::UniswapV3 => 3,
            },
            format!("{:?}", self.token0),
            format!("{:?}", self.token1),
            self.decimals0,
            self.decimals1,
            self.fee,
        )
    }
}

pub async fn load_all_pools_from_v2(
    wss_url: String,
    factory_addresses: Vec<H160>,
    from_blocks: Vec<u64>,
) -> Result<Vec<Pool>> {
    let file_path = FilePath::new("src/.cached-pools.csv");

    if file_path.exists() {
        info!("Cargando pools desde el caché: {:?}", file_path.to_str().unwrap());
        let mut reader = csv::Reader::from_path(file_path)?;
        let pools_vec: Vec<Pool> = reader.records().filter_map(|row| row.ok()).map(Pool::from).collect();
        info!("Se han cargado {} pools desde el caché.", pools_vec.len());
        return Ok(pools_vec);
    }

    info!("No se encontró caché. Sincronizando pools desde la blockchain...");
    let ws = Ws::connect(wss_url).await?;
    let provider = Arc::new(Provider::new(ws));
    let mut dexes_data = Vec::new();

    for (i, address) in factory_addresses.iter().enumerate() {
        dexes_data.push((*address, CfmmsDexVariant::UniswapV2, from_blocks[i]));
    }

    let dexes: Vec<_> = dexes_data
        .into_iter()
        .map(|(address, variant, block_number)| Dex::new(address, variant, block_number, Some(3000)))
        .collect();

    let pools_cfmms: Vec<CfmmsPool> = sync_pairs(dexes.clone(), provider.clone(), None).await?;

    let pools_set: BTreeSet<Pool> = pools_cfmms
        .into_iter()
        .filter_map(|pool| match pool {
            CfmmsPool::UniswapV2(pool) => Some(Pool {
                address: pool.address,
                version: DexVariant::UniswapV2,
                token0: pool.token_a,
                token1: pool.token_b,
                decimals0: pool.token_a_decimals,
                decimals1: pool.token_b_decimals,
                fee: pool.fee,
            }),
            CfmmsPool::UniswapV3(_) => None, // Ignoramos pools V3 en este bot
        })
        .collect();
    
    let pools_vec: Vec<Pool> = pools_set.into_iter().collect();
    info!("Pools sincronizadas desde blockchain: {}", pools_vec.len());

    let mut writer = csv::Writer::from_path(file_path)?;
    writer.write_record(&[
        "address", "version", "token0", "token1", "decimals0", "decimals1", "fee",
    ])?;

    for pool in &pools_vec {
        writer.serialize(pool.cache_row())?;
    }
    writer.flush()?;
    info!("Pools guardadas en caché para futuros usos.");

    Ok(pools_vec)
  }
          
