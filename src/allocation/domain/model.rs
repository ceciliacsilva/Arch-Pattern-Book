use std::collections::HashSet;

use anyhow::{Result, bail};

use thiserror::Error;

use super::events::{Allocated, Deallocated, Event, OutOfStock};

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("Error in Product {0}")]
struct ProductError(String);

/// A `Product` is something that can be `allocated`.
#[derive(Debug, Default)]
pub struct Product {
    pub sku: String,
    pub batches: Vec<Batch>,
    pub version_number: i32,
    pub events: Vec<super::events::Event>,
}

impl Product {
    pub(crate) fn new(sku: String, version_number: i32, batches: Vec<Batch>) -> Self {
        Self {
            sku,
            batches,
            version_number,
            events: vec![],
        }
    }

    pub(crate) fn add_batch(&mut self, batch: Batch) {
        self.batches.push(batch);
    }

    pub(crate) fn allocate(&mut self, line: OrderLine) -> Result<String> {
        self.batches.sort();

        if let Some(batch) = self.batches.iter_mut().find(|b| b.can_allocate(&line)) {
            batch.allocate(line.clone());
            self.version_number += 1;
            self.events.push(Event::Allocated(Allocated::new(
                line.order_id.clone(),
                line.sku.clone(),
                line.qty,
                batch.reference.clone(),
            )));
            return Ok(batch.reference.clone());
        }

        self.events
            .push(Event::OutOfStock(OutOfStock::new(line.sku)));

        bail!(ProductError(
            "Unable to find a batch to allocate".to_string()
        ));
    }

    pub(crate) fn change_batch_quantity(&mut self, reference: String, qty: i32) {
        if let Some(batch) = self.batches.iter_mut().find(|b| b.reference == reference) {
            batch.stock_quantity = qty;
            while batch.available_quantity() < 0 {
                if let Some(line) = batch.deallocate_one() {
                    self.events.push(Event::Deallocated(Deallocated::new(
                        line.order_id,
                        line.sku,
                        line.qty,
                    )));
                }
                // TODO: what do we in the `None` case? Python ignores this possibility
            }
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub(crate) struct OrderLine {
    pub(crate) order_id: String,
    pub(crate) sku: String,
    pub(crate) qty: i32,
}

impl OrderLine {
    pub(crate) fn new(order_id: String, sku: String, qty: i32) -> Self {
        Self { order_id, sku, qty }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Batch {
    pub(crate) reference: String,
    pub(crate) sku: String,
    pub(crate) eta: Option<time::OffsetDateTime>,
    pub(crate) stock_quantity: i32,
    pub(crate) allocations: HashSet<OrderLine>,
}

impl Batch {
    pub(crate) fn new(
        reference: String,
        sku: String,
        eta: Option<time::OffsetDateTime>,
        stock_quantity: i32,
    ) -> Self {
        Self {
            reference,
            sku,
            eta,
            stock_quantity,
            allocations: HashSet::new(),
        }
    }

    fn allocated_quantity(&self) -> i32 {
        self.allocations.iter().map(|line| line.qty).sum()
    }

    fn available_quantity(&self) -> i32 {
        self.stock_quantity - self.allocated_quantity()
    }

    pub(crate) fn can_allocate(&self, line: &OrderLine) -> bool {
        self.sku == line.sku && self.available_quantity() >= line.qty
    }

    pub(crate) fn allocate(&mut self, line: OrderLine) {
        if self.can_allocate(&line) {
            self.allocations.insert(line);
        }
    }

    pub(crate) fn deallocate_one(&mut self) -> Option<OrderLine> {
        self.allocations.drain().next()
    }
}

impl PartialEq for Batch {
    /// Two batches are the same if they have the same `reference`.
    fn eq(&self, other: &Self) -> bool {
        self.reference == other.reference
    }
}

impl Eq for Batch {}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd for Batch {
    /// The order between Batches is defined by they `eta`.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.eta.partial_cmp(&other.eta)
    }
}

impl Ord for Batch {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.eta.cmp(&other.eta)
    }
}

#[cfg(test)]
mod tests {
    use crate::allocation::domain::events::{Allocated, Event, OutOfStock};

    use super::{Batch, OrderLine, Product};

    fn tomorrow() -> time::OffsetDateTime {
        time::OffsetDateTime::now_utc() + time::Duration::days(1)
    }

    fn later() -> time::OffsetDateTime {
        time::OffsetDateTime::now_utc() + time::Duration::days(10)
    }

    #[test]
    fn test_prefer_warehouse_batches_to_shipment_and_emits_events() {
        // RefCell to check `available_quantity`.
        let in_stock_batch = Batch::new(
            "in-stock-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            None,
            100,
        );
        let shipment_batch = Batch::new(
            "shipment-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            Some(tomorrow()),
            100,
        );

        let mut product = Product::new(
            "RETRO-CLOCK".to_string(),
            0,
            vec![in_stock_batch, shipment_batch],
        );
        let line = OrderLine::new("oref".to_string(), "RETRO-CLOCK".to_string(), 10);

        let batch_ref = product.allocate(line.clone()).unwrap();

        assert_eq!(batch_ref, "in-stock-batch".to_string());

        let mut batches = product.batches.iter();
        assert_eq!(
            batches
                .next()
                .expect("Should be in-stock-batch")
                .available_quantity(),
            90
        );
        assert_eq!(
            batches
                .next()
                .expect("Should be shipment-batch")
                .available_quantity(),
            100
        );

        assert!(
            product.events.pop()
                == Some(Event::Allocated(Allocated::new(
                    line.order_id,
                    line.sku,
                    10,
                    batch_ref
                )))
        );
    }

    #[test]
    fn test_prefer_earlier_batches() {
        // RefCell to check `available_quantity`.
        let speedy_batch = Batch::new(
            "speedy-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            None,
            100,
        );
        let normal_batch = Batch::new(
            "normal-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            Some(tomorrow()),
            100,
        );

        let slow_batch = Batch::new(
            "slow-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            Some(later()),
            100,
        );

        let mut product = Product::new(
            "RETRO-CLOCK".to_string(),
            0,
            vec![speedy_batch, normal_batch, slow_batch],
        );
        let line = OrderLine::new("oref".to_string(), "RETRO-CLOCK".to_string(), 10);

        let batch_ref = product.allocate(line).unwrap();

        assert_eq!(batch_ref, "speedy-batch".to_string());

        let mut batches = product.batches.iter();
        assert_eq!(
            batches
                .next()
                .expect("Should be speedy-batch")
                .available_quantity(),
            90
        );
        assert_eq!(
            batches
                .next()
                .expect("Should be normal-batch")
                .available_quantity(),
            100
        );
        assert_eq!(
            batches
                .next()
                .expect("Should be slow-batch")
                .available_quantity(),
            100
        );
    }

    #[test]
    fn test_records_out_of_stock_events_if_cannot_allocate() {
        // RefCell to check `available_quantity`.
        let less_qty_batch = Batch::new(
            "less-qty-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            None,
            5,
        );

        let mut product = Product::new("RETRO-CLOCK".to_string(), 0, vec![less_qty_batch]);
        let line = OrderLine::new("oref".to_string(), "RETRO-CLOCK".to_string(), 10);

        let batch_ref = product.allocate(line.clone());

        assert!(batch_ref.is_err());

        let mut batches = product.batches.iter();
        assert_eq!(
            batches
                .next()
                .expect("Should be less-qty-batch")
                .available_quantity(),
            5
        );

        assert!(product.events.pop() == Some(Event::OutOfStock(OutOfStock::new(line.sku))));
    }

    #[test]
    fn test_increment_version_number() {
        // RefCell to check `available_quantity`.
        let in_stock_batch = Batch::new(
            "in-stock-batch".to_string(),
            "RETRO-CLOCK".to_string(),
            None,
            100,
        );

        let mut product = Product::new("RETRO-CLOCK".to_string(), 0, vec![in_stock_batch]);
        let line = OrderLine::new("oref".to_string(), "RETRO-CLOCK".to_string(), 10);

        let batch_ref = product.allocate(line.clone()).unwrap();

        assert_eq!(batch_ref, "in-stock-batch".to_string());

        let mut batches = product.batches.iter();
        assert_eq!(
            batches
                .next()
                .expect("Should be in-stock-batch")
                .available_quantity(),
            90
        );

        assert!(product.version_number == 1);
    }
}
