-- Add migration script here

CREATE TABLE
    erc20_transaction_logs (
        transaction_hash TEXT PRIMARY KEY NOT NULL UNIQUE,
        block_number INTEGER NOT NULL,
        symbol TEXT NOT NULL,
        value TEXT NOT NULL,
        to_address TEXT NOT NULL,
        from_address TEXT NOT NULL,
        created_at TIMESTAMP NOT NULL DEFAULT NOW()
    );