use crate::allocation::adapters::repository::Repository;
use crate::allocation::{
    adapters::repository::SqlxRepository,
    domain::{
        commands::*,
        events::*,
        model::{Batch, OrderLine},
    },
};
use redis::aio::MultiplexedConnection;

use anyhow::Result;
use tracing::debug;

pub async fn allocate(cmd: Allocate, r: &mut SqlxRepository) -> Result<()> {
    let line = OrderLine::new(cmd.order_id, cmd.sku, cmd.qty);
    let product = r.get(&line.sku).await?;
    let mut product = product.lock().await;
    let _ = product.allocate(line.clone())?;

    // This can fail if product is updated by someone else or if `change_batch_quantity`.
    // We can decide to re-try
    r.reconcile_db(&product).await?;

    Ok(())
}

pub async fn change_batch_qty(cmd: ChangeBatchQuantity, r: &mut SqlxRepository) -> Result<()> {
    let product = r.get_by_batch_ref(&cmd.reference).await?;
    let mut product = product.lock().await;
    product.change_batch_quantity(cmd.reference, cmd.qty);

    r.reconcile_db(&product).await?;

    Ok(())
}

pub async fn add_batch(cmd: CreateBatch, r: &mut SqlxRepository) -> Result<()> {
    debug!("adding batch");
    let product = r.get(&cmd.sku).await?;
    debug!("product: {product:?}");
    let mut product = product.lock().await;
    let batch = Batch::new(cmd.reference, cmd.sku, cmd.eta, cmd.qty);
    product.add_batch(batch);
    debug!("here");

    r.reconcile_db(&product).await?;

    Ok(())
}

pub async fn add_allocation_to_read_model(event: &Allocated, r: &mut SqlxRepository) -> Result<()> {
    r.add_allocation_to_allocationview(&event.order_id, &event.sku, &event.batch_ref)
        .await;

    Ok(())
}

pub async fn publish_allocated_event(event: &Allocated, conn: MultiplexedConnection) -> Result<()> {
    crate::allocation::adapters::redis_eventpublisher::publish(conn, event).await
}

pub async fn reallocate(event: &Deallocated, r: &mut SqlxRepository) -> Result<()> {
    let cmd = Allocate::new(&event.order_id, &event.sku, event.qty);
    allocate(cmd, r).await
}

pub async fn delete_allocation_to_read_model(
    event: &Deallocated,
    r: &mut SqlxRepository,
) -> Result<()> {
    r.delete_allocation_to_allocationview(&event.order_id, &event.sku)
        .await;

    Ok(())
}
