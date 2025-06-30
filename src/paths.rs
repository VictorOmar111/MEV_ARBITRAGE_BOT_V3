// src/paths.rs

use ethers::types::{H160, U256};
use log::info;
use std::{collections::HashMap, time::Instant};

use crate::multi::Reserve;
use crate::pools::Pool;
use crate::simulator::UniswapV2Simulator;

#[derive(Debug, Clone)]
pub struct ArbPath {
    pub nhop: u8,
    pub pool_1: Pool,
    pub pool_2: Pool,
    pub pool_3: Pool,
    pub token_a: H160,
    pub token_b: H160,
    pub token_c: H160,
    pub zero_for_one_1: bool,
    pub zero_for_one_2: bool,
    pub zero_for_one_3: bool,
}

impl ArbPath {
    fn _get_pool(&self, i: u8) -> &Pool {
        match i {
            0 => &self.pool_1,
            1 => &self.pool_2,
            2 => &self.pool_3,
            _ => panic!("Invalid pool index in path"),
        }
    }

    fn _get_zero_for_one(&self, i: u8) -> bool {
        match i {
            0 => self.zero_for_one_1,
            1 => self.zero_for_one_2,
            2 => self.zero_for_one_3,
            _ => panic!("Invalid zero_for_one index in path"),
        }
    }

    pub fn simulate_v2_path(
        &self,
        amount_in: U256,
        reserves: &HashMap<H160, Reserve>,
    ) -> Option<U256> {
        let mut amount_out = amount_in;
        for i in 0..self.nhop {
            let pool = self._get_pool(i);
            let zero_for_one = self._get_zero_for_one(i);
            let reserve = reserves.get(&pool.address)?;
            let (reserve_in, reserve_out) = if zero_for_one {
                (reserve.reserve0, reserve.reserve1)
            } else {
                (reserve.reserve1, reserve.reserve0)
            };
            amount_out = UniswapV2Simulator::get_amount_out(
                amount_out,
                reserve_in,
                reserve_out,
                U256::from(pool.fee),
            )?;
        }
        Some(amount_out)
    }

    pub fn get_swap_path(&self) -> Vec<H160> {
        vec![self.token_a, self.token_b, self.token_c, self.token_a]
    }
}

pub fn generate_triangular_paths(pools: &Vec<Pool>, _token_in: H160) -> Vec<ArbPath> {
    info!("Generando rutas de arbitraje triangular...");
    let start_time = Instant::now();
    let mut paths = Vec::new();

    let mut pools_by_token: HashMap<H160, Vec<&Pool>> = HashMap::new();
    for pool in pools {
        pools_by_token.entry(pool.token0).or_default().push(pool);
        pools_by_token.entry(pool.token1).or_default().push(pool);
    }

    for pool_ab in pools {
        let (token_a, token_b) = (pool_ab.token0, pool_ab.token1);

        if let Some(candidate_pools_bc) = pools_by_token.get(&token_b) {
            for &pool_bc in candidate_pools_bc {
                if pool_bc.address == pool_ab.address { continue; }
                
                let token_c = if pool_bc.token0 == token_b { pool_bc.token1 } else { pool_bc.token0 };

                if token_c == token_a { continue; }

                if let Some(candidate_pools_ca) = pools_by_token.get(&token_c) {
                    for &pool_ca in candidate_pools_ca {
                        if pool_ca.address == pool_ab.address || pool_ca.address == pool_bc.address { continue; }

                        if (pool_ca.token0 == token_c && pool_ca.token1 == token_a) || (pool_ca.token1 == token_c && pool_ca.token0 == token_a) {
                            
                            let path = ArbPath {
                                nhop: 3,
                                pool_1: pool_ab.clone(),
                                pool_2: pool_bc.clone(),
                                pool_3: pool_ca.clone(),
                                token_a,
                                token_b,
                                token_c,
                                zero_for_one_1: token_a == pool_ab.token0,
                                zero_for_one_2: token_b == pool_bc.token0,
                                zero_for_one_3: token_c == pool_ca.token0,
                            };
                            paths.push(path);
                        }
                    }
                }
            }
        }
    }

    info!(
        "Generadas {} rutas de 3 saltos en {:.2} segundos.",
        paths.len(),
        start_time.elapsed().as_secs_f64()
    );

    paths
                                 }
                              
