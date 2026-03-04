use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{debug, error};

use crate::allocation::domain::events::Event;
use crate::allocation::domain::model::Product;
use crate::allocation::domain::orm::Mapper;

pub trait Repository {
    async fn get(&mut self, sku: &str) -> Result<Arc<Mutex<Product>>>;
    async fn get_by_batch_ref(&mut self, batch_ref: &str) -> Result<Arc<Mutex<Product>>>;
    async fn collect_new_events(&mut self) -> Vec<Event>;
    async fn reconcile_db(&mut self, product: &Product) -> Result<()>;

    async fn add_allocation_to_allocationview(
        &mut self,
        order_id: &str,
        sku: &str,
        batch_ref: &str,
    );
    async fn delete_allocation_to_allocationview(&mut self, order_id: &str, sku: &str);
}

pub struct SqlxRepository {
    pub connection_pool: PgPool,
    pub seem: Vec<Arc<Mutex<Product>>>,
}

impl SqlxRepository {
    pub fn new(connection_pool: PgPool) -> Self {
        Self {
            connection_pool,
            seem: vec![],
        }
    }
}

impl Repository for SqlxRepository {
    async fn get(&mut self, sku: &str) -> Result<Arc<Mutex<Product>>> {
        let product = Product::select_by_sku(sku, &self.connection_pool).await?;

        let product_ref = Arc::new(Mutex::new(product));
        self.seem.push(Arc::clone(&product_ref));
        Ok(product_ref)
    }

    async fn get_by_batch_ref(&mut self, reference: &str) -> Result<Arc<Mutex<Product>>> {
        let product = Product::select_by_batch_reference(reference, &self.connection_pool).await?;

        let product_ref = Arc::new(Mutex::new(product));
        self.seem.push(Arc::clone(&product_ref));
        Ok(product_ref)
    }

    async fn reconcile_db(&mut self, product: &Product) -> Result<()> {
        let mut txn = self.connection_pool.begin().await?;
        debug!("reconcile db state for product: {product:?}");

        let reconcile_p = product.reconcile_db(&mut txn).await;

        match reconcile_p {
            Ok(_) => {
                txn.commit().await?;
                return Ok(());
            }
            Err(error) => error!("Error during reconciliation, {error:?}"),
        }

        txn.rollback().await?;
        Ok(())
    }

    async fn add_allocation_to_allocationview(
        &mut self,
        order_id: &str,
        sku: &str,
        batch_ref: &str,
    ) {
        let _ = sqlx::query!(
            "INSERT INTO allocationsview(order_id, sku, batch_ref) values ($1, $2, $3)",
            order_id,
            sku,
            batch_ref
        )
        .execute(&self.connection_pool)
        .await;
    }

    async fn delete_allocation_to_allocationview(&mut self, order_id: &str, sku: &str) {
        let _ = sqlx::query!(
            "DELETE FROM allocationsview WHERE order_id = $1 AND sku = $2",
            order_id,
            sku,
        )
        .execute(&self.connection_pool)
        .await;
    }

    async fn collect_new_events(&mut self) -> Vec<Event> {
        let mut events_all: Vec<Event> = vec![];
        for product in self.seem.clone() {
            let mut product = product.lock().await;
            let mut events = product.events.clone();
            product.events.clear();

            events_all.append(&mut events);
        }

        events_all
    }
}
