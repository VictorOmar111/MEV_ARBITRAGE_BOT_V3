// src/paths.rs

use ethers::providers::Middleware;
use ethers::types::{H160, U256};
use log::{info, warn};
use std::collections::HashMap;
use std::time::Instant;

use crate::pools::{DexVariant, Pool};
use crate::simulator::UniswapV3Simulator;

/// Representa una ruta de arbitraje triangular completa a través de 3 pools de Uniswap V3.
#[derive(Debug, Clone)]
pub struct ArbPath {
    pub pool_1: Pool,
    pub pool_2: Pool,
    pub pool_3: Pool,
    pub token_a: H160, // Token de inicio y fin del ciclo
    pub token_b: H160, // Token intermedio 1
    pub token_c: H160, // Token intermedio 2
    // Banderas para la dirección del swap en cada pool (true si es token0 -> token1)
    pub zero_for_one_1: bool,
    pub zero_for_one_2: bool,
    pub zero_for_one_3: bool,
}

impl ArbPath {
    /// Simula la ejecución de los 3 swaps en cadena llamando al Uniswap V3 Quoter.
    /// Devuelve la cantidad final del token de salida (`token_a`) si la simulación es exitosa.
    pub async fn simulate_v3_path<M: Middleware>(
        &self,
        amount_in: U256,
        provider: &M,
        quoter: H160,
    ) -> Option<U256> {
        let mut current_amount = amount_in;
        let mut current_token_in = self.token_a;

        // Iteramos a través de los 3 pasos del arbitraje
        for (pool, token_out, _zero_for_one) in [
            (&self.pool_1, self.token_b, self.zero_for_one_1),
            (&self.pool_2, self.token_c, self.zero_for_one_2),
            (&self.pool_3, self.token_a, self.zero_for_one_3),
        ] {
            let amount_out = UniswapV3Simulator::quote_exact_input_single(
                provider,
                quoter,
                current_token_in,
                token_out,
                pool.fee.into(),
                current_amount,
            )
            .await
            .ok()?; // Si cualquier quote falla, la simulación entera falla.

            current_amount = amount_out;
            current_token_in = token_out;
        }
        
        Some(current_amount)
    }

    /// Construye el path de tokens (A → B → C → A) para ser usado en la transacción del smart contract.
    pub fn get_swap_path(&self) -> Vec<H160> {
        vec![self.token_a, self.token_b, self.token_c, self.token_a]
    }
}

/// Genera todas las rutas de arbitraje triangular únicas usando exclusivamente pools V3 con 0.01% de fee.
pub fn generate_triangular_paths(pools: &[Pool], token_in: H160) -> Vec<ArbPath> {
    info!("🔍 Generando rutas triangulares V3 0.01%...");
    let start_time = Instant::now();

    // Filtra primero para trabajar solo con el universo de pools que nos interesa.
    let pools_v3_001: Vec<&Pool> = pools
        .iter()
        .filter(|p| p.version == DexVariant::UniswapV3 && p.fee == 100)
        .collect();
    
    info!("Encontrados {} pools de Uniswap V3 con 0.01% de fee.", pools_v3_001.len());

    let mut pools_by_token: HashMap<H160, Vec<&Pool>> = HashMap::new();
    for p in &pools_v3_001 {
        pools_by_token.entry(p.token0).or_default().push(p);
        pools_by_token.entry(p.token1).or_default().push(p);
    }

    let mut paths = Vec::new();

    if let Some(first_pools) = pools_by_token.get(&token_in) {
        for &pool_ab in first_pools {
            let token_b = if pool_ab.token0 == token_in { pool_ab.token1 } else { pool_ab.token0 };

            if let Some(second_pools) = pools_by_token.get(&token_b) {
                for &pool_bc in second_pools {
                    if pool_bc.address == pool_ab.address { continue; }
                    
                    let token_c = if pool_bc.token0 == token_b { pool_bc.token1 } else { pool_bc.token0 };
                    if token_c == token_in || token_c == token_b { continue; }

                    if let Some(third_pools) = pools_by_token.get(&token_c) {
                        for &pool_ca in third_pools {
                            if pool_ca.address == pool_ab.address || pool_ca.address == pool_bc.address { continue; }
                            
                            let closes_loop = (pool_ca.token0 == token_c && pool_ca.token1 == token_in) || 
                                              (pool_ca.token1 == token_c && pool_ca.token0 == token_in);

                            if closes_loop {
                                paths.push(ArbPath {
                                    pool_1: (*pool_ab).clone(),
                                    pool_2: (*pool_bc).clone(),
                                    pool_3: (*pool_ca).clone(),
                                    token_a: token_in,
                                    token_b,
                                    token_c,
                                    zero_for_one_1: pool_ab.token0 == token_in,
                                    zero_for_one_2: pool_bc.token0 == token_b,
                                    zero_for_one_3: pool_ca.token0 == token_c,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let elapsed_secs = start_time.elapsed().as_secs_f64();
    info!(
        "➡️ Generadas {} rutas en {:.2}s → {:.2} rutas/s",
        paths.len(),
        elapsed_secs,
        paths.len() as f64 / elapsed_secs
    );

    paths
}
