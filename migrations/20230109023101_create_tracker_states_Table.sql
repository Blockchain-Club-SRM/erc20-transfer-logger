-- Add migration script here
CREATE TABLE tracker_state (
  name TEXT PRIMARY KEY,
  block_number INTEGER,
  updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
  CHECK (block_number>=0)
);
