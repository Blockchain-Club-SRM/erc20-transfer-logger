-- Add migration script here

CREATE TABLE
    erc20_transaction_logs (
        id SERIAL PRIMARY KEY,
        transaction_hash CHAR(66) NOT NULL,
        block_number INTEGER NOT NULL,
        symbol TEXT NOT NULL,
        value TEXT NOT NULL,
        to_address CHAR(42) NOT NULL,
        from_address CHAR(42) NOT NULL,
        created_at TIMESTAMP NOT NULL DEFAULT NOW(),
        UNIQUE (transaction_hash, block_number, symbol, to_address, from_address)
    );