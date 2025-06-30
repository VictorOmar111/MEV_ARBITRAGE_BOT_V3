// src/simulator.rs
use ethers::types::U256;

pub struct UniswapV2Simulator;

impl UniswapV2Simulator {
    /// Calcula el `amountOut` para un swap de Uniswap V2, incluyendo la comisión.
    pub fn get_amount_out(
        amount_in: U256,
        reserve_in: U256,
        reserve_out: U256,
        _fee: U256, // En V2, la fee está implícita en el 997/1000
    ) -> Option<U256> {
        if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
            return None;
        }

        let amount_in_with_fee = amount_in * 997;
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * 1000 + amount_in_with_fee;

        Some(numerator / denominator)
    }
}
