use crate::currency::Currency;
use crate::id::{AllocationId, PotId};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pot {
    pub id: PotId,
    pub name: String,
    pub currency: Currency,
    pub target: Option<Decimal>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Allocation {
    pub id: AllocationId,
    pub pot_id: PotId,
    pub amount: Decimal,
    pub date: NaiveDate,
    pub note: Option<String>,
}
