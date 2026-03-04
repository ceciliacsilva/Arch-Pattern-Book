-- Add migration script here
CREATE TABLE IF NOT EXISTS AllocationsView (
       order_id VARCHAR NOT NULL,
       sku VARCHAR NOT NULL,
       batch_ref VARCHAR NOT NULL
);
