use time::OffsetDateTime;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Allocate(Allocate),
    CreateBatch(CreateBatch),
    ChangeBatchQuantity(ChangeBatchQuantity),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Allocate {
    pub order_id: String,
    pub sku: String,
    pub qty: i32,
}
impl Allocate {
    pub(crate) fn new(order_id: &str, sku: &str, qty: i32) -> Self {
        Self {
            order_id: order_id.to_string(),
            sku: sku.to_string(),
            qty,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CreateBatch {
    pub reference: String,
    pub sku: String,
    pub qty: i32,
    pub eta: Option<time::OffsetDateTime>,
}

impl CreateBatch {
    pub fn new(reference: &str, sku: &str, qty: i32, eta: Option<OffsetDateTime>) -> Self {
        Self {
            reference: reference.to_owned(),
            sku: sku.to_owned(),
            qty,
            eta,
        }
    }
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
pub struct ChangeBatchQuantity {
    pub reference: String,
    pub qty: i32,
}
