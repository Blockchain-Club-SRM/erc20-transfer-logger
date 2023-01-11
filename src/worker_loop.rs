use anyhow::Context;
use ethers::{
    abi::AbiDecode,
    providers::{Middleware, Provider, StreamExt, Ws},
    types::{Address, Filter, Log, H256, U256, U64, Bytes},
    utils::format_units,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::{
    configuration::Settings, constants::IERC20, startup::get_connection_pool,
    tracker_client::TrackerClient,
};

pub async fn run_worker_until_stopped(configuration: Settings) -> Result<(), anyhow::Error> {
    let connection_pool = get_connection_pool(&configuration.database);
    let tracker_client = TrackerClient::new(configuration.tracker.url).await;
    let mut token_data: HashMap<Address, (String, u8)> = HashMap::new();
    worker_loop(&connection_pool, tracker_client, &mut token_data).await
}

async fn worker_loop(
    connection_pool: &PgPool,
    tracker_client: TrackerClient,
    token_data: &mut HashMap<Address, (String, u8)>,
) -> Result<(), anyhow::Error> {
    let mut last_block = get_last_block(connection_pool, tracker_client.client.clone()).await?;
    loop {
        match try_execute_task(connection_pool, tracker_client.client.clone(), &last_block, token_data).await {
            Ok(ExecutionOutcome::EmptyQueue) => {
                println!("Emtpy Queue");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                println!("Error: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Ok(ExecutionOutcome::TaskCompleted) => {
                println!(">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>");
                last_block = last_block + 1;
            }
        }
    }
}
pub enum ExecutionOutcome {
    TaskCompleted,
    EmptyQueue,
}

pub async fn try_execute_task(
    pool: &PgPool,
    client: Arc<Provider<Ws>>,
    last_block: &U64,
    token_data: &mut HashMap<Address, (String, u8)>,
) -> Result<ExecutionOutcome, anyhow::Error> {
    println!("next block: {}", &last_block);
    let erc20_transfer_filter = Filter::new()
        .from_block(last_block)
        .event("Transfer(address,address,uint256)");
    let mut batch: Vec<(String, String, Address, Address, U64, H256)> = vec![];
    let mut stream = client.get_logs_paginated(&erc20_transfer_filter, 10);
    while let Some(res) = stream.next().await {
        if let Ok(log) = res {
            if check_valid_erc20_transaction(&log) {
                let data = token_data.get(&log.address);
                let (symbol, decimals) = match data {
                    Some((a, b)) => (a.clone(), b.clone()),
                    None => {
                        let contract = get_contract(log.address, client.clone());
                        let symbol = get_symbol(&contract).await;
                        let decimals = get_decimal(&contract).await;
                        token_data.insert(log.address, (symbol.clone(), decimals.clone()));
                        (symbol, decimals)
                    }
                };
                let amount = get_readable_amount(&log, &decimals).await;
                let from = get_address(log.topics[1]);
                let to = get_address(log.topics[2]);
                if let Some(block_number) = log.block_number {
                    let block_number = block_number;
                    if let Some(transaction_hash) = log.transaction_hash {
                        batch.push((amount, symbol.clone(), from, to, block_number, transaction_hash));
                    }
                }
                println!("Block {} Transaction's in Batch {} ", last_block, batch.len().to_string());
            }
        }
    }
    if batch.is_empty() {
        return Ok(ExecutionOutcome::EmptyQueue);
    }
    let mut transaction = pool.begin().await.context("Begin Error")?;
    insert_batch_erc20_transaction_data(&mut transaction, &batch, &last_block).await?;
    save_last_block(&mut transaction, &last_block)
        .await?;
    transaction.commit().await.context("Commit Error")?;
    println!("<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<");
    Ok(ExecutionOutcome::TaskCompleted)
}

pub async fn get_last_block(
    pool: &PgPool,
    client: Arc<Provider<Ws>>,
) -> Result<U64, anyhow::Error> {
    match sqlx::query!(
        r#"
            SELECT * FROM tracker_state
            where name = $1"#,
        "erc20"
    )
    .fetch_one(pool)
    .await
    {
        Ok(block) => {
            if let Some(number) = block.block_number {
                let number = U64::try_from(number)?;
                Ok(number)
            } else {
                Ok(client.get_block_number().await?)
            }
        }
        Err(_) => Ok(client.get_block_number().await?),
    }
}

pub async fn save_last_block(
    transaction: &mut Transaction<'_, Postgres>,
    block_number: &U64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"
            INSERT INTO tracker_state (name, block_number)
            VALUES ($1, $2)
            ON CONFLICT (name) DO UPDATE SET block_number = $2"#,
        "erc20",
        U64::as_u64(block_number) as i32
    )
    .execute(transaction)
    .await?;
    Ok(())
}
pub fn get_contract(address: Address, client: Arc<Provider<Ws>>) -> IERC20<Provider<Ws>> {
    IERC20::new(address, client.clone())
}

pub async fn get_symbol(contract: &IERC20<Provider<Ws>>) -> String {
    match contract.symbol().call().await {
        Ok(symbol) => symbol,
        Err(_) => "Unknown".to_string(),
    }
}

pub async fn get_decimal(contract: &IERC20<Provider<Ws>>) -> u8 {
    match contract.decimals().call().await {
        Ok(decimal) => decimal,
        Err(_) => 1u8,
    }
}

pub async fn get_readable_amount(log: &Log, decimals: &u8) -> String {
    if let Ok(amount) = U256::decode(&log.data) {
        if let Ok(readable_amount) = format_units(amount, decimals.clone() as i32) {
            return readable_amount;
        } else {
            return U256::to_string(&amount);
        }
    } else {
        return Bytes::to_string(&log.data);
    }
}

pub fn get_address(address: H256) -> Address {
    Address::from(address)
}

pub fn check_valid_erc20_transaction(log: &Log) -> bool {
    log.topics.len() == 3  && log.data != vec![0u8; 32]
}

pub async fn insert_batch_erc20_transaction_data(
    transaction: &mut Transaction<'_, Postgres>,
    data: &Vec<(String, String, Address, Address, U64, H256)>,
    last_block: &U64,
) -> Result<(), anyhow::Error> {
    let mut v1: Vec<String> = Vec::new();
    let mut v2: Vec<String> = Vec::new();
    let mut v3: Vec<_> = Vec::new();
    let mut v4: Vec<_> = Vec::new();
    let mut v5: Vec<i32> = Vec::new();
    let mut v6: Vec<_> = Vec::new();
    for d in data.iter() {
        v1.push(d.0.to_string());
        v2.push(d.1.to_string());
        v3.push(d.2.as_bytes());
        v4.push(d.3.as_bytes());
        v5.push(U64::as_u64(last_block) as i32);
        v6.push(d.5.as_bytes());
    }
    println!("v1: {}", v1.len());
    sqlx::query(
        r#"
        INSERT INTO erc20_transaction_logs (value, symbol, from_address, to_address, block_number, transaction_hash)
        SELECT * FROM UNNEST($1, $2, $3, $4, $5, $6)
        ON CONFLICT
        DO NOTHING"#,
    )
    .bind(&v1)
    .bind(&v2)
    .bind(&v3)
    .bind(&v4)
    .bind(&v5)
    .bind(&v6)
    .execute(transaction)
    .await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}