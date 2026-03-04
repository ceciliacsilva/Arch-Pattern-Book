pub(crate) mod allocation;
pub(crate) mod bootstrap;

use allocation::{
    domain::commands::{Allocate, Command, CreateBatch},
    service_layer::message_bus::Message,
};
use axum::{
    Json, Router,
    extract::{self, Path, State},
    http::StatusCode,
    routing::{get, post},
};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPool, prelude::FromRow};
use time::OffsetDateTime;
use tokio::net::TcpListener;
use tracing::{debug, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_connection_str = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://allocation:abc123@localhost/allocations".to_string());

    info!("Connection string found.");

    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:63791".to_string());

    info!("Redis url found.");

    let (pool_read, service_bus_sender) =
        bootstrap::init(&db_connection_str, &redis_url, &["change_batch_quantity"])
            .await
            .expect("initialization failed");

    let app = Router::new()
        .route("/add_batch", post(add_batch))
        .route("/allocate", post(allocate))
        .with_state(service_bus_sender)
        .route("/allocations/{order_id}", get(allocations_view))
        .with_state(pool_read);

    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    debug!("Listening on {}", listener.local_addr().unwrap());

    let _ = axum::serve(listener, app).await;
}

#[derive(Debug, Clone, Deserialize)]
struct BatchDTO {
    sku: String,
    reference: String,
    qty: i32,
    eta: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize)]
struct Response {
    batch_ref: String,
    msg: String,
}

async fn add_batch(
    State(queue_tx): State<tokio::sync::mpsc::Sender<Message>>,
    extract::Json(batch): extract::Json<BatchDTO>,
) -> Result<Json<Response>, (StatusCode, String)> {
    let create_batch = Command::CreateBatch(CreateBatch::new(
        &batch.reference,
        &batch.sku,
        batch.qty,
        batch.eta,
    ));

    let _ = queue_tx.send(Message::Command(create_batch)).await;

    Ok(Json(Response {
        batch_ref: batch.reference.clone(),
        msg: "Batch added to the database".to_string(),
    }))
}

#[derive(Debug, Clone, Deserialize)]
struct AllocateDTO {
    order_id: String,
    sku: String,
    qty: i32,
}

async fn allocate(
    State(queue_tx): State<tokio::sync::mpsc::Sender<Message>>,
    extract::Json(request): extract::Json<AllocateDTO>,
) -> Result<Json<Response>, (StatusCode, String)> {
    let allocate = Command::Allocate(Allocate::new(&request.order_id, &request.sku, request.qty));

    let _ = queue_tx.send(Message::Command(allocate)).await;

    Ok(Json(Response {
        batch_ref: request.order_id.clone(),
        msg: "Allocation completed".to_string(),
    }))
}

#[derive(Debug, Clone, Serialize)]
struct AllocationView {
    allocations: Vec<Allocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct Allocation {
    order_id: String,
    sku: String,
    batch_ref: String,
}

async fn allocations_view(
    Path(order_id): Path<String>,
    State(pg_pool): State<PgPool>,
) -> Result<Json<AllocationView>, (StatusCode, String)> {
    let sql = "SELECT
                sku,
                batch_ref
            FROM allocationsview WHERE order_id = $1;";

    let stream = sqlx::query_as::<_, Allocation>(sql)
        .bind(order_id)
        .fetch(&pg_pool);
    let allocations = stream.try_collect().await.map_err(internal_error)?;

    Ok(Json(AllocationView { allocations }))
}

/// Utility function for mapping any error into a `500 Internal Server Error`
/// response.
fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
