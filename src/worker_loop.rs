use anyhow::Context;
use ethers::{
    abi::AbiDecode,
    providers::{Middleware, Provider, StreamExt, Ws},
    types::{Address, Filter, Log, H160, H256, U256, U64},
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
    let mut token_data: HashMap<String, (String, i32)> = HashMap::new();
    worker_loop(&connection_pool, tracker_client, &mut token_data).await
}

async fn worker_loop(
    connection_pool: &PgPool,
    tracker_client: TrackerClient,
    token_data: &mut HashMap<String, (String, i32)>,
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
                println!("hey");
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
    token_data: &mut HashMap<String, (String, i32)>,
) -> Result<ExecutionOutcome, anyhow::Error> {
    println!("next block: {}", &last_block);
    let erc20_transfer_filter = Filter::new()
        .from_block(last_block)
        .event("Transfer(address,address,uint256)");
    let mut batch: Vec<(String, String, String, String, U64, String)> = vec![];
    let mut stream = client.get_logs_paginated(&erc20_transfer_filter, 10);
    while let Some(res) = stream.next().await {
        if let Ok(log) = res {
            if check_valid_erc20_transaction(&log) {
                let data = token_data.get(&log.address.to_string());
                let (symbol, decimals) = match data {
                    Some((a, b)) => (a.clone(), b.clone()),
                    None => {
                        let contract = get_contract(log.address, client.clone());
                        let symbol = get_symbol(&contract).await;
                        let decimals = get_decimal(&contract).await;
                        token_data.insert(log.address.to_string(), (symbol.clone(), decimals));
                        (symbol, decimals)
                    }
                };
                let amount = get_readable_amount(&log, decimals).await;
                let from = get_address(log.topics[1]);
                let to = get_address(log.topics[2]);
                if let Some(block_number) = log.block_number {
                    let block_number = block_number;
                    if let Some(transaction_hash) = log.transaction_hash {
                        let transaction_hash = transaction_hash.to_string();
                        batch.push((amount, symbol, from, to, block_number, transaction_hash));
                    }
                }
                println!("in batch {} ", batch.len().to_string());
            }
        }
    }
    if batch.is_empty() {
        return Ok(ExecutionOutcome::EmptyQueue);
    }
    let mut transaction = pool.begin().await.context("Begin Error")?;
    insert_batch_erc20_transaction_data(&mut transaction, &batch).await?;
    save_last_block(&mut transaction, last_block.clone())
        .await
        .context("Insert Last Block Error")?;
    transaction.commit().await.context("Commit Error")?;
    println!("done");
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
    block_number: U64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"
            INSERT INTO tracker_state (name, block_number)
            VALUES ($1, $2)
            ON CONFLICT (name) DO UPDATE SET block_number = $2"#,
        "erc20",
        U64::as_u64(&block_number) as i32,
    )
    .execute(transaction)
    .await?;
    Ok(())
}
pub fn get_contract(address: H160, client: Arc<Provider<Ws>>) -> IERC20<Provider<Ws>> {
    IERC20::new(address, client.clone())
}

pub async fn get_symbol(contract: &IERC20<Provider<Ws>>) -> String {
    match contract.symbol().call().await {
        Ok(symbol) => symbol,
        Err(_) => "Unknown".to_string(),
    }
}

pub async fn get_decimal(contract: &IERC20<Provider<Ws>>) -> i32 {
    match contract.decimals().call().await {
        Ok(decimal) => decimal as i32,
        Err(_) => 1,
    }
}

pub async fn get_readable_amount(log: &Log, decimals: i32) -> String {
    if let Ok(amount) = U256::decode(&log.data) {
        if let Ok(readable_amount) = format_units(amount, decimals) {
            return readable_amount;
        } else {
            return amount.to_string();
        }
    } else {
        return log.data.to_string();
    }
}

pub fn get_address(address: H256) -> String {
    Address::from(address).to_string()
}

pub fn check_valid_erc20_transaction(log: &Log) -> bool {
    if log.topics.len() == 3 {
        true
    } else {
        false
    }
}

pub async fn insert_batch_erc20_transaction_data(
    transaction: &mut Transaction<'_, Postgres>,
    data: &Vec<(String, String, String, String, U64, String)>,
) -> Result<(), anyhow::Error> {
    let mut v1: Vec<String> = Vec::new();
    let mut v2: Vec<String> = Vec::new();
    let mut v3: Vec<String> = Vec::new();
    let mut v4: Vec<String> = Vec::new();
    let mut v5: Vec<i32> = Vec::new();
    let mut v6: Vec<String> = Vec::new();
    for d in data.iter() {
        v1.push(d.0.to_string());
        v2.push(d.1.to_string());
        v3.push(d.2.to_string());
        v4.push(d.3.to_string());
        v5.push(U64::as_u64(&d.4) as i32);
        v6.push(d.5.to_string());
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

// pub fn add_unique_transaction_to_batch(
//     data: (String, String, String, String, U64, String),
//     batch: &mut Vec<(String, String, String, String, U64, String)>,
// ) -> &mut Vec<(String, String, String, String, U64, String)> {
//     let mut is_exist = false;
//     for d in batch.iter() {
//         if d.5 == data.5 {
//             is_exist = true;
//             break;
//         }
//     }
//     if !is_exist {
//         batch.push(data);
//     }
//     batch
// }
