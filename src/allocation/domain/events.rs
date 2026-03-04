/// An `Event` is a fact about things that have happened.
/// TODO: discuss about why an `enum` and not a `trait object`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum Event {
    Allocated(Allocated),
    Deallocated(Deallocated),
    OutOfStock(OutOfStock),
}

/// An `Event` that states `an allocating has happend for order_id in batch_ref`.
#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize)]
pub(crate) struct Allocated {
    pub(crate) order_id: String,
    pub(crate) sku: String,
    // We should have negative quantities.
    pub(crate) qty: i32,
    pub(crate) batch_ref: String,
}

impl Allocated {
    pub(crate) fn new(order_id: String, sku: String, qty: i32, batch_ref: String) -> Self {
        Self {
            order_id,
            sku,
            qty,
            batch_ref,
        }
    }
}

/// An `Event` that states `a deallocating has happend for order_id in batch_ref`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Deallocated {
    pub order_id: String,
    pub sku: String,
    pub qty: i32,
}

impl Deallocated {
    pub(crate) fn new(order_id: String, sku: String, qty: i32) -> Self {
        Self { order_id, sku, qty }
    }
}

/// An `Event` that states `item {sku} is out of stock`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct OutOfStock {
    sku: String,
}

impl OutOfStock {
    pub(crate) fn new(sku: String) -> Self {
        Self { sku }
    }
}
