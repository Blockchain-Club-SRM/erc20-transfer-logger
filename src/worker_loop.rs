use std::{ sync::Arc, time::Duration};
use anyhow::Context;
use ethers::{
    abi::AbiDecode,
    providers::{Middleware, Provider, StreamExt, Ws},
    types::{Address, Filter, Log, H160, H256, U256, U64},
    utils::format_units,
};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    configuration::Settings, constants::IERC20, startup::get_connection_pool,
    tracker_client::TrackerClient,
};
// type PgTransaction = Transaction<'static, Postgres>;
struct ParsedTransactionData {
    symbol: String,
    amount: String,
    from_address: String,
    to_address: String,
    block_number: U64,
    transaction_hash: String,
}
pub async fn run_worker_until_stopped(configuration: Settings) -> Result<(), anyhow::Error> {
    let connection_pool = get_connection_pool(&configuration.database);
    let tracker_client = TrackerClient::new(configuration.tracker.url).await;
    worker_loop(&connection_pool, tracker_client).await
}

async fn worker_loop(
    connection_pool: &PgPool,
    tracker_client: TrackerClient,
) -> Result<(), anyhow::Error> {
    let mut last_block = get_last_block(connection_pool, tracker_client.client.clone()).await?;
    println!("next block: {}", last_block);
    loop {
        match try_execute_task(connection_pool, tracker_client.client.clone(), &last_block)
            .await
        {
            Ok(ExecutionOutcome::EmptyQueue) => {
                println!("Emtpy Queue");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                println!("Error: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Ok(ExecutionOutcome::TaskCompleted) => {
                last_block= last_block + 1;
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
) -> Result<ExecutionOutcome, anyhow::Error> {
    let erc20_transfer_filter = Filter::new()
        .from_block(last_block)
        .event("Transfer(address,address,uint256)");
        let mut batch: Vec<ParsedTransactionData> = vec![];

        let mut stream = client.get_logs_paginated(&erc20_transfer_filter, 100);
        while let Some(res) = stream.next().await {
            if let Ok(log) = res {
                if check_valid_erc20_transaction(&log) {
                    let contract = get_contract(log.address, client.clone());
                    let symbol = get_symbol(&contract).await;
                    let decimals = get_decimal(&contract).await;
                    let amount = get_readable_amount(&log, decimals).await;
                    let from = get_address(log.topics[1]);
                    let to = get_address(log.topics[2]);
                    let block_number = log.block_number.unwrap();
                    let transaction_hash = log.transaction_hash.unwrap().to_string();
                    batch.push(ParsedTransactionData {
                        symbol,
                        amount,
                        from_address: from,
                        to_address: to,
                        block_number,
                        transaction_hash,
                    });
                }
            }
        }
        if batch.is_empty() {
            return Ok(ExecutionOutcome::EmptyQueue);
        }
        println!("Batch Size: {}", batch.len());
        let mut transaction = pool.begin().await.context("Begin Error")?;
        insert_batch_erc20_transaction_data(&mut transaction, &batch).await?;
        save_last_block(&mut transaction, batch.last().unwrap().block_number).await.context("Insert Last Block Error")?;
        transaction.commit().await.context("Commit Error")?;
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

async fn insert_batch_erc20_transaction_data(
    transaction: &mut Transaction<'_, Postgres>,
    data: &Vec<ParsedTransactionData>,
) -> Result<(), anyhow::Error> {
    let mut values = String::new();
    for (i, d) in data.iter().enumerate() {
        values.push_str(&format!(
            "({}, {}, {}, {}, {}, {})",
            d.amount,
            d.symbol,
            d.from_address,
            d.to_address,
            U64::as_u64(&d.block_number) as i32,
            d.transaction_hash
        ));
        if i != data.len() - 1 {
            values.push_str(",");
        }
    }
    dbg!(&values);
    sqlx::query(
        r#"
        INSERT INTO erc20_transaction_data (value, symbol, from_address, to_address, block_number, transaction_hash)
        VALUES
        "#,
    )
    .bind(&values)
    .execute(transaction)
    .await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}
