use crate::id::{AccountId, ValuationId};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Valuation {
    pub id: ValuationId,
    pub account_id: AccountId,
    pub date: NaiveDate,
    pub old_value: Decimal,
    pub new_value: Decimal,
    pub category: String,
}

impl Valuation {
    pub fn gain(&self) -> Decimal {
        self.new_value - self.old_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn gain_is_the_difference_between_old_and_new_value() {
        let v = Valuation {
            id: ValuationId::generate(),
            account_id: AccountId::generate(),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            old_value: dec!(1000),
            new_value: dec!(1042),
            category: "Investment gain".to_string(),
        };
        assert_eq!(v.gain(), dec!(42));
    }

    #[test]
    fn a_loss_is_a_negative_gain() {
        let v = Valuation {
            id: ValuationId::generate(),
            account_id: AccountId::generate(),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            old_value: dec!(1000),
            new_value: dec!(950),
            category: "Investment gain".to_string(),
        };
        assert_eq!(v.gain(), dec!(-50));
    }
}
