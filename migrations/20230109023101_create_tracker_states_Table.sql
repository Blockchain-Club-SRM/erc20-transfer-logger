-- Add migration script here
CREATE TABLE tracker_state (
  name TEXT PRIMARY KEY,
  block_number INTEGER,
  CHECK (block_number>=0)
);
