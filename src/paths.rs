use crate::{
    oracle::OracleMap,
    simulator,
    types::{Pool, DexVariant},
};
use anyhow::Result;
use ethers::{
    prelude::*,
    types::{H160, U256},
};
use log::info;
use std::{cmp::Ordering, collections::HashMap, sync::Arc, time::Instant};

// --- Constantes de Filtrado del Pathfinder ---
// Ignorar pools con menos de $50k de liquidez para evitar alto slippage.
const MIN_TVL_USD: f64 = 50_000.0;
// Limitar el número de pools por token para evitar una explosión combinatoria.
const MAX_POOLS_PER_TOKEN: usize = 75;

/// Representa una ruta de arbitraje triangular completa A -> B -> C -> A.
#[derive(Debug, Clone)]
pub struct ArbPath {
    pub pool_1: Pool,
    pub pool_2: Pool,
    pub pool_3: Pool,
    pub token_a: H160,
    pub token_b: H160,
    pub token_c: H160,
    pub score: f64, // El score se calculará y asignará en el módulo de optimización.
}

impl ArbPath {
    pub fn key(&self) -> String {
        format!("{:?}-{:?}-{:?}", self.pool_1.address, self.pool_2.address, self.pool_3.address)
    }
    /// Simula un arbitraje a través de los 3 pools de la ruta.
    /// Toma una cantidad de `token_a` y devuelve la cantidad final de `token_a`.
    pub async fn simulate_v3_path<M: Middleware + 'static>(
        &self,
        provider: Arc<M>,
        amount_in: U256,
    ) -> Option<U256> {
        // Salto 1: A -> B
        let (token_in_1, token_out_1) = if self.pool_1.token0 == self.token_a {
            (self.pool_1.token0, self.pool_1.token1)
        } else {
            (self.pool_1.token1, self.pool_1.token0)
        };
        let amount_out_1 = simulator::quote_exact_input_single(
            provider.clone(), self.pool_1.version, token_in_1, token_out_1, self.pool_1.fee, amount_in,
        ).await.ok()?;

        if amount_out_1.is_zero() { return None; }

        // Salto 2: B -> C
        let (token_in_2, token_out_2) = if self.pool_2.token0 == self.token_b {
            (self.pool_2.token0, self.pool_2.token1)
        } else {
            (self.pool_2.token1, self.pool_2.token0)
        };
        let amount_out_2 = simulator::quote_exact_input_single(
            provider.clone(), self.pool_2.version, token_in_2, token_out_2, self.pool_2.fee, amount_out_1,
        ).await.ok()?;

        if amount_out_2.is_zero() { return None; }

        // Salto 3: C -> A
        let (token_in_3, token_out_3) = if self.pool_3.token0 == self.token_c {
            (self.pool_3.token0, self.pool_3.token1)
        } else {
            (self.pool_3.token1, self.pool_3.token0)
        };
        let final_amount_out = simulator::quote_exact_input_single(
            provider, self.pool_3.version, token_in_3, token_out_3, self.pool_3.fee, amount_out_2,
        ).await.ok()?;

        Some(final_amount_out)
    }

    /// Obtiene el precio spot aproximado de la ruta simulando con 1 unidad del token de entrada.
    pub async fn get_spot_price<M: Middleware + 'static>(&self, provider: Arc<M>) -> Result<f64> {
        let input_decimals = self.get_input_decimals();
        let one_token = U256::from(10).pow(U256::from(input_decimals));

        let simulated_out = self.simulate_v3_path(provider, one_token).await.unwrap_or_default();

        Ok(simulated_out.as_u128() as f64 / 10f64.powi(input_decimals as i32))
    }

    /// Devuelve los decimales del token de entrada (token_a) de la ruta.
    pub fn get_input_decimals(&self) -> u8 {
        if self.pool_1.token0 == self.token_a {
            self.pool_1.decimals0
        } else {
            self.pool_1.decimals1
        }
    }

    // Funciones de conveniencia para acceder a datos anidados.
    pub fn address(&self, index: usize) -> H160 {
        match index {
            1 => self.pool_1.address,
            2 => self.pool_2.address,
            3 => self.pool_3.address,
            _ => H160::zero(),
        }
    }
}

/// Genera todas las rutas de arbitraje triangular (A->B->C->A) a partir de una lista de pools.
pub fn generate_triangular_paths(
    pools: &[Pool],
    token_in: H160,
    oracle_map: &OracleMap,
) -> Vec<ArbPath> {
    let start_time = Instant::now();
    info!(" Generando rutas triangulares (TVL >= ${}, top {} pools/token)...", MIN_TVL_USD, MAX_POOLS_PER_TOKEN);

    // 1. Filtrar pools por TVL mínimo.
    let filtered_pools: Vec<&Pool> = pools.iter().filter(|p| p.tvl_usd >= MIN_TVL_USD).collect();

    // 2. Agrupar pools por cada token que contienen.
    let mut pools_by_token: HashMap<H160, Vec<&Pool>> = HashMap::new();
    for pool in &filtered_pools {
        pools_by_token.entry(pool.token0).or_default().push(pool);
        pools_by_token.entry(pool.token1).or_default().push(pool);
    }

    // 3. Para cada token, mantener solo los N pools más líquidos para optimizar.
    for list in pools_by_token.values_mut() {
        list.sort_unstable_by(|a, b| b.tvl_usd.partial_cmp(&a.tvl_usd).unwrap_or(Ordering::Equal));
        list.truncate(MAX_POOLS_PER_TOKEN);
    }

    let mut valid_paths = Vec::new();
    // 4. Construir las rutas A -> B -> C -> A.
    if let Some(first_hop_pools) = pools_by_token.get(&token_in) {
        for &pool_ab in first_hop_pools {
            let token_b = if pool_ab.token0 == token_in { pool_ab.token1 } else { pool_ab.token0 };

            // Filtro inteligente: no continuar si el token intermedio no tiene oráculo.
            if oracle_map.get_feeds(&token_b).is_none() { continue; }

            if let Some(second_hop_pools) = pools_by_token.get(&token_b) {
                for &pool_bc in second_hop_pools {
                    if pool_bc.address == pool_ab.address { continue; } // Evitar usar el mismo pool dos veces.

                    let token_c = if pool_bc.token0 == token_b { pool_bc.token1 } else { pool_bc.token0 };

                    if token_c == token_in { continue; } // Evitar rutas A->B->A
                    if oracle_map.get_feeds(&token_c).is_none() { continue; }

                    if let Some(third_hop_pools) = pools_by_token.get(&token_c) {
                        for &pool_ca in third_hop_pools {
                            if pool_ca.address == pool_ab.address || pool_ca.address == pool_bc.address { continue; }

                            // Verificar que el tercer pool cierra el ciclo de vuelta a token_in.
                            let closes_loop = (pool_ca.token0 == token_c && pool_ca.token1 == token_in)
                                || (pool_ca.token1 == token_c && pool_ca.token0 == token_in);

                            if closes_loop {
                                valid_paths.push(ArbPath {
                                    pool_1: (*pool_ab).clone(),
                                    pool_2: (*pool_bc).clone(),
                                    pool_3: (*pool_ca).clone(),
                                    token_a: token_in,
                                    token_b,
                                    token_c,
                                    score: 0.0,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    info!(" Rutas generadas: {} en {:.2}s", valid_paths.len(), start_time.elapsed().as_secs_f64());
    valid_paths
}
