use anyhow::{Result, bail};
use futures::TryStreamExt;
use sqlx::{FromRow, PgPool, PgTransaction};
use thiserror::Error;
use time::OffsetDateTime;

use super::model::{Batch, Product};

#[derive(Debug, Clone, FromRow)]
pub(crate) struct ProductDB {
    sku: String,
    version_number: i32,
}

impl ProductDB {
    fn into(self, batches: Vec<Batch>) -> Product {
        Product::new(self.sku, self.version_number, batches)
    }
}

#[derive(Debug, Clone, FromRow)]
struct BatchDB {
    reference: String,
    sku: String,
    purchased_quantity: i32,
    eta: OffsetDateTime,
}

impl From<BatchDB> for Batch {
    fn from(value: BatchDB) -> Self {
        Batch::new(
            value.reference,
            value.sku,
            Some(value.eta),
            value.purchased_quantity,
        )
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MapperError {
    #[error("Couldn't allocate product due to contention")]
    ProductVersionUpdated(i32),
}

pub(crate) trait Mapper {
    type To;
    async fn select_by_sku(sku: &str, conn: &PgPool) -> Result<Self::To>;
    async fn select_by_batch_reference(reference: &str, conn: &PgPool) -> Result<Self::To>;
    async fn reconcile_db(&self, txn: &mut PgTransaction<'_>) -> Result<()>;
}

impl Mapper for Product {
    type To = Product;

    async fn select_by_sku(sku: &str, conn: &PgPool) -> Result<Self::To> {
        let sql = "select * from product p where p.sku = $1";
        let product_db = sqlx::query_as::<_, ProductDB>(sql)
            .bind(sku)
            .fetch_one(conn)
            .await?;

        let sql = "select * from batch b where b.sku = $1";

        let batches_db: Vec<BatchDB> = sqlx::query_as::<_, BatchDB>(sql)
            .bind(sku)
            .fetch(conn)
            .try_collect()
            .await?;

        let batches: Vec<Batch> = batches_db.into_iter().map(|r| r.into()).collect();
        let product: Product = product_db.into(batches);

        Ok(product)
    }

    async fn select_by_batch_reference(reference: &str, conn: &PgPool) -> Result<Self::To> {
        let sql = " select p.* from product p join batch b on (p.sku = b.sku) where b.reference=$1";

        let product_db = sqlx::query_as::<_, ProductDB>(sql)
            .bind(reference)
            .fetch_one(conn)
            .await?;

        let sql = "select * from batch b where b.reference = $1";

        let batches_db: Vec<BatchDB> = sqlx::query_as::<_, BatchDB>(sql)
            .bind(reference)
            .fetch(conn)
            .try_collect()
            .await?;

        let batches: Vec<Batch> = batches_db.into_iter().map(|r| r.into()).collect();
        let product: Product = product_db.into(batches);

        Ok(product)
    }

    async fn reconcile_db(&self, txn: &mut PgTransaction<'_>) -> Result<()> {
        let db_vn: i32 = sqlx::query_scalar("SELECT version_number FROM product WHERE sku = $1")
            .bind(self.sku.clone())
            .fetch_one(txn.as_mut())
            .await?;

        if self.version_number != db_vn {
            bail!(MapperError::ProductVersionUpdated(db_vn));
        }

        sqlx::query!(
            "UPDATE PRODUCT SET version_number = $1 WHERE sku = $2",
            self.version_number,
            self.sku
        )
        .execute(txn.as_mut())
        .await?;

        // XXX: This is a upsert.
        // Create `Batch` if a `Batch` with `reference` doesn't exists.
        // Update the `Batch` if it already exists.
        for batch in self.batches.clone() {
            sqlx::query!(
                "INSERT INTO
                      batch(reference, sku, purchased_quantity, eta)
                      VALUES ($1, $2, $3, $4) ON CONFLICT(reference)
                DO UPDATE SET purchased_quantity = $3",
                batch.reference,
                batch.sku,
                batch.stock_quantity,
                if let Some(eta) = batch.eta {
                    eta
                } else {
                    OffsetDateTime::now_utc()
                },
            )
            .execute(txn.as_mut())
            .await?;

            for allocation in batch.allocations {
                sqlx::query!(
                    "INSERT INTO
                      ORDERLINE(sku, qty, order_id)
                      VALUES ($1, $2, $3)",
                    allocation.sku,
                    allocation.qty,
                    allocation.order_id,
                )
                .execute(txn.as_mut())
                .await?;
            }
        }

        Ok(())
    }
}
