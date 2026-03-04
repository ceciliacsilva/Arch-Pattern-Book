-- Add migration script here
CREATE TABLE IF NOT EXISTS Product (
       sku VARCHAR PRIMARY KEY,
       version_number INTEGER NOT NULL
);
