-- Add migration script here

CREATE TABLE
    erc20_transaction_logs (
        transaction_hash VARCHAR(66) PRIMARY KEY NOT NULL UNIQUE,
        block_number INTEGER NOT NULL,
        symbol VARCHAR(10) NOT NULL,
        value TEXT NOT NULL,
        to_address VARCHAR(42) NOT NULL,
        from_address VARCHAR(42) NOT NULL,
        created_at TIMESTAMP NOT NULL DEFAULT NOW()
    );