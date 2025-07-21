use crate::constants::{PANCAKESWAP_V3_QUOTER, SUSHISWAP_V3_QUOTER, UNISWAP_V3_QUOTER};
use crate::types::DexVariant;
use anyhow::Result;
use ethers::{
    prelude::*,
    types::{H160, U256},
};
use std::sync::Arc;

// CORRECCIÓN FINAL: El ABI debe listar los parámetros de forma individual, no dentro de un `params` struct.
abigen!(
    IQuoterV2,
    r#"[{"name":"quoteExactInputSingle","type":"function","stateMutability":"nonpayable","inputs":[{"name":"tokenIn","type":"address"},{"name":"tokenOut","type":"address"},{"name":"fee","type":"uint24"},{"name":"amountIn","type":"uint256"},{"name":"sqrtPriceLimitX96","type":"uint160"}],"outputs":[{"name":"amountOut","type":"uint256"}]}]"#,
);

pub fn get_quoter_address(variant: DexVariant) -> H160 {
    match variant {
        DexVariant::UniswapV3 => *UNISWAP_V3_QUOTER,
        DexVariant::SushiV3 => *SUSHISWAP_V3_QUOTER,
        DexVariant::PancakeV3 => *PANCAKESWAP_V3_QUOTER,
    }
}

pub async fn quote_exact_input_single<M: Middleware + 'static>(
    provider: Arc<M>,
    variant: DexVariant,
    token_in: H160,
    token_out: H160,
    fee: u32,
    amount_in: U256,
) -> Result<U256> {
    let quoter_address = get_quoter_address(variant);
    let quoter = IQuoterV2::new(quoter_address, provider);

    // CORRECCIÓN FINAL: Los parámetros se pasan directamente a la función.
    let amount_out = quoter
        .quote_exact_input_single(token_in, token_out, fee, amount_in, U256::zero())
        .call()
        .await?;

    Ok(amount_out)
}
