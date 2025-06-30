// src/execution.rs

use anyhow::{Error, Result};
use ethers::{
    abi::Abi,
    prelude::*,
    types::{Address, Bytes, TxHash, U256},
};
use log::info;
use std::sync::Arc;

use crate::config::CONFIG;
use crate::paths::ArbPath;
use crate::pools::DexVariant;

fn string_to_bytes32(s: &str) -> Result<[u8; 32]> {
    let mut bytes = [0u8; 32];
    let s_bytes = s.as_bytes();
    if s_bytes.len() > 32 {
        return Err(Error::msg("La string es demasiado larga para bytes32"));
    }
    bytes[..s_bytes.len()].copy_from_slice(s_bytes);
    Ok(bytes)
}

fn select_router_key(path: &ArbPath) -> &'static str {
    match path.pool_1.version {
        DexVariant::UniswapV2 => "UniswapV2",
        DexVariant::UniswapV3 => "UniswapV3",
    }
}

fn calculate_amount_out_min(expected_out: U256, slippage_bps: u32) -> U256 {
    let one = U256::from(10_000u32);
    expected_out * (one - U256::from(slippage_bps)) / one
}

pub async fn execute_flashloan_transaction(
    path: &ArbPath,
    optimal_amount_in: U256,
    expected_output: U256,
) -> Result<TxHash> {
    info!("--- Iniciando Proceso de Ejecución de Transacción ---");

    let config = &*CONFIG;
    let provider = Provider::<Http>::try_from(config.https_url.clone())?;
    let chain_id = config.chain_id;
    let wallet: LocalWallet = config.private_key.parse::<LocalWallet>()?.with_chain_id(chain_id);
    let client = Arc::new(SignerMiddleware::new(provider, wallet.clone()));

    let contract_address = config.contract_address;
    let contract_abi: Abi = serde_json::from_str(include_str!("../ArbBotV2_abi.json"))?;
    let contract = Contract::new(contract_address, contract_abi, client.clone());

    let router_key_str = select_router_key(path);
    let router_key_bytes32 = string_to_bytes32(router_key_str)?;
    let trade_path_tokens: Vec<Address> = path.get_swap_path();
    let amount_out_min = calculate_amount_out_min(expected_output, 50); // 0.5% slippage
    let bribe_info = U256::zero();

    use ethers::abi::Token;
    let user_data_tokens = vec![
        Token::FixedBytes(router_key_bytes32.to_vec()),
        Token::Array(trade_path_tokens.into_iter().map(Token::Address).collect()),
        Token::Uint(amount_out_min),
        Token::Uint(bribe_info),
    ];
    let user_data = ethers::abi::encode(&user_data_tokens);

    let loan_token = path.token_a;
    let loan_amount = optimal_amount_in;
    
    let call = contract.method::<_, ()>("startArb", (loan_token, loan_amount, Bytes::from(user_data)))?;

    info!("Transacción preparada. Enviando...");
    let pending_tx = client.send_transaction(call.tx, None).await?;
    info!("Transacción enviada. Hash: {:?}", pending_tx.tx_hash());

    let receipt = pending_tx
        .await?
        .ok_or_else(|| Error::msg("La transacción no fue minada"))?;
        
    info!("¡Transacción minada con éxito! Recibo: {:?}", receipt.transaction_hash);

    Ok(receipt.transaction_hash)
}
