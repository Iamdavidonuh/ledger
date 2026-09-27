use crate::currency::Currency;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pot {
    pub id: Uuid,
    pub name: String,
    pub currency: Currency,
    pub target: Option<Decimal>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Allocation {
    pub id: Uuid,
    pub pot_id: Uuid,
    pub amount: Decimal,
    pub date: NaiveDate,
    pub note: Option<String>,
}
