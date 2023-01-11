-- Add migration script here

CREATE TABLE
    erc20_transaction_logs (
        id SERIAL PRIMARY KEY,
        transaction_hash TEXT NOT NULL,
        block_number INTEGER NOT NULL,
        symbol bytea NOT NULL,
        value DECIMAL(30,10) NOT NULL,
        to_address TEXT NOT NULL,
        from_address TEXT NOT NULL,
        created_at TIMESTAMP NOT NULL DEFAULT NOW(),
        UNIQUE (transaction_hash, block_number, symbol, to_address, from_address)
    );