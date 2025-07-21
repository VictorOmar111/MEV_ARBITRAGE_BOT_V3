use crate::{
    config::CONFIG,
    constants::WETH_ADDRESS,
    optimization::ArbitrageOpportunity,
    oracle::OracleMap,
    paths::ArbPath,
    provider,
};
use anyhow::{anyhow, Error, Result};
use chrono::Local;
use ethers::{prelude::*, types::transaction::eip2718::TypedTransaction, abi::Token};
use log::{error, info, warn};
use std::sync::Arc;
use tokio::task::JoinSet;

abigen!(IArbitrageBot, "./abi/ArbitrageBotV4_abi.json");

fn generate_session_id() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    U256::from(rand::random::<u128>()).to_big_endian(&mut bytes);
    bytes
}
fn calculate_amount_out_min(expected_amount: U256, slippage_bps: u32) -> U256 {
    let basis_points = U256::from(10_000);
    if slippage_bps >= 10_000 { return U256::zero(); }
    let slippage = U256::from(slippage_bps);
    expected_amount * (basis_points - slippage) / basis_points
}
fn deadline_from_now_aggressive() -> U256 {
    U256::from(Local::now().timestamp() as u64 + 25)
}
pub fn encode_arb_data(
    path: &ArbPath, expected_output: U256, slippage_bps: u32,
) -> Result<Bytes> {
    let mut path_bytes = Vec::new();
    path_bytes.extend_from_slice(path.token_a.as_bytes());
    path_bytes.extend_from_slice(&path.pool_1.fee.to_be_bytes()[1..]);
    path_bytes.extend_from_slice(path.token_b.as_bytes());
    path_bytes.extend_from_slice(&path.pool_2.fee.to_be_bytes()[1..]);
    path_bytes.extend_from_slice(path.token_c.as_bytes());
    let amount_out_min = calculate_amount_out_min(expected_output, slippage_bps);
    let arb_data_tuple = Token::Tuple(vec![
        Token::Bytes(path_bytes),
        Token::FixedBytes(generate_session_id().to_vec()),
        Token::Uint(deadline_from_now_aggressive()),
        Token::Uint(amount_out_min),
    ]);
    Ok(ethers::abi::encode(&[arb_data_tuple]).into())
}
pub async fn execute_arbitrage_bundle(
    client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    opportunities: Vec<ArbitrageOpportunity>,
    base_fee: U256,
) -> Vec<Result<(TxHash, String), (anyhow::Error, String)>> {
    info!(" Ejecutando bundle con {} oportunidades...", opportunities.len());
    let mut set = JoinSet::new();
    for opp in opportunities {
        let client_clone = client.clone();
        let path_key = opp.path.key();
        set.spawn(async move {
            match execute_single_transaction(client_clone, opp, base_fee).await {
                Ok(tx_hash) => Ok((tx_hash, path_key)),
                Err(e) => Err((e, path_key)),
            }
        });
    }
    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(result) = res { results.push(result); }
    }
    results
}
pub async fn execute_single_transaction(
    client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    opp: ArbitrageOpportunity,
    base_fee: U256,
) -> Result<TxHash> {
    if opp.optimal_amount_in.is_zero() || opp.expected_output <= opp.optimal_amount_in {
        return Err(Error::msg("Monto inválido o no rentable."));
    }
    let contract = IArbitrageBot::new(CONFIG.contract_address, client.clone());
    let user_data = encode_arb_data(&opp.path, opp.expected_output, opp.slippage_bps)?;
    let call = contract.start_flashloan_arbitrage(opp.path.token_a, opp.optimal_amount_in, user_data);

    // CORRECCIÓN FINAL: Clonamos `call.tx` para evitar el error de "partial move".
    let mut tx: TypedTransaction = call.tx.clone();
    tx.set_chain_id(CONFIG.chain_id);
    tx.set_gas(provider::estimate_gas(&call).await?);

    let oracle_map = Arc::new(OracleMap::new());
    let eth_price = oracle_map.get_price(&*WETH_ADDRESS, client.provider().clone().into()).await.ok_or_else(|| anyhow!("Failed to get ETH price"))?.price;
    let bribe_in_eth = opp.bribe_usd / eth_price;
    let mut priority_fee_in_gwei = (bribe_in_eth * 1e9) as u64;
    for attempt in 0..3 {
        if attempt > 0 {
            warn!("Reintento de TX #{}: aumentando priority_fee...", attempt + 1);
            priority_fee_in_gwei = (priority_fee_in_gwei as f64 * 1.5) as u64;
        }
        let priority_fee = U256::from(priority_fee_in_gwei) * U256::exp10(9);
        let max_fee_per_gas = base_fee + priority_fee;
        if let Some(eip1559) = tx.as_eip1559_mut() {
            eip1559.max_fee_per_gas = Some(max_fee_per_gas);
            eip1559.max_priority_fee_per_gas = Some(priority_fee);
        }
        match client.send_transaction(tx.clone(), None).await {
            Ok(pending) => {
                let tx_hash = pending.tx_hash();
                info!(" TX enviada con éxito! Hash: {tx_hash:?}");
                return Ok(tx_hash);
            }
            Err(e) if attempt < 2 => {
                error!("Error en envío de TX (intento {}): {:?}. Reintentando...", attempt + 1, e);
                tokio::time::sleep(std::time::Duration::from_millis(150 * (attempt + 1))).await;
            }
            Err(e) => return Err(Error::msg(format!("TX falló tras 3 intentos: {e}"))),
        }
    }
    Err(Error::msg("Lógica de reintentos de envío de TX falló."))
}
