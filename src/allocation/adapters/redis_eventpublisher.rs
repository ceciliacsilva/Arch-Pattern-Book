use tracing::info;

use crate::allocation::domain::events::Allocated;

use anyhow::Result;
use redis::{AsyncTypedCommands, aio::MultiplexedConnection};

pub async fn publish(mut conn: MultiplexedConnection, event: &Allocated) -> Result<()> {
    info!("publishing: channel={:?}, event={event:?}", conn);

    let allocated = serde_json::to_string(event)?;

    let _num_sub = conn.publish("line_allocated", allocated).await;
    Ok(())
}
