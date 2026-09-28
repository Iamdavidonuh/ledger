use crate::currency::Currency;
use crate::id::AccountId;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Own,
    Outside,
    Person,
    Investment,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub name: String,
    pub currency: Currency,
    pub kind: AccountKind,
    pub opening_balance: Decimal,
    pub archived: bool,
    pub current_value: Option<Decimal>,
}

impl Account {
    pub fn new(
        name: impl Into<String>,
        currency: Currency,
        kind: AccountKind,
        opening_balance: Decimal,
    ) -> Self {
        Account {
            id: AccountId::generate(),
            name: name.into(),
            currency,
            kind,
            opening_balance,
            archived: false,
            current_value: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn a_new_account_gets_a_unique_id_and_starts_unarchived() {
        let eur = Currency::new("EUR").unwrap();
        let a = Account::new("Checking", eur.clone(), AccountKind::Own, dec!(0));
        let b = Account::new("Checking", eur, AccountKind::Own, dec!(0));
        assert_ne!(a.id, b.id);
        assert!(!a.archived);
        assert_eq!(a.opening_balance, dec!(0));
        assert_eq!(a.current_value, None);
    }
}
