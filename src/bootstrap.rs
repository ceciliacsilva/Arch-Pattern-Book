use std::time::Duration;

use crate::allocation::{
    adapters::repository::SqlxRepository,
    domain::commands::*,
    service_layer::message_bus::{Message, MessageBus},
};
use anyhow::Result;
use futures_util::StreamExt;
use redis::aio::{MultiplexedConnection, PubSubStream};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::sync::mpsc::Sender;
use tracing::info;

/// Bootstrap init Redis and DB Connection pool.
pub(crate) async fn init(
    db_connection_str: &str,
    redis_url: &str,
    redis_channel_sub: &[&str],
) -> Result<(PgPool, Sender<Message>)> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(db_connection_str)
        .await?;
    info!("Connection pool to DB created");

    let client = redis::Client::open(redis_url)?;
    // conn will be used to publish msg.
    let conn: MultiplexedConnection = client.get_multiplexed_async_connection().await?;
    info!("Redis connection to publish created");

    let client = redis::Client::open(redis_url)?;
    // sub_receiver will be used to receive msg from subscribed channels.
    let (mut sink, sub_receiver) = client.get_async_pubsub().await?.split();
    info!("Redis connection to subcriber created");

    let _ = sink.subscribe(redis_channel_sub).await;

    info!("Redis subscribed to change_batch_quantity");

    let (sender, receiver) = tokio::sync::mpsc::channel(100);

    tokio::spawn(bootstrap_redis_consumer(sub_receiver, sender.clone()));
    info!("Redis subscriber ready");

    let repository = SqlxRepository::new(pool);
    info!("Repository created");

    let messagebus = MessageBus::new(repository, receiver, conn);
    tokio::spawn(messagebus.init());

    info!("Connection with DB established");

    let pool_read = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(db_connection_str)
        .await
        .expect("can't connect to database");

    Ok((pool_read, sender))
}

async fn bootstrap_redis_consumer(
    mut stream: PubSubStream,
    queue_tx: tokio::sync::mpsc::Sender<crate::allocation::service_layer::message_bus::Message>,
) -> anyhow::Result<()> {
    while let Some(msg) = stream.next().await {
        let payload: String = msg.get_payload()?;
        let change_batch_qty: ChangeBatchQuantity = serde_json::from_str(&payload)?;
        let _ = queue_tx
            .send(Message::Command(Command::ChangeBatchQuantity(
                change_batch_qty,
            )))
            .await;
    }

    Ok(())
}
