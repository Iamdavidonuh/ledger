use crate::account::Account;
use crate::entry::{Entry, EntryPart};
use crate::error::LedgerError;
use crate::pot::{Allocation, Pot};
use crate::valuation::Valuation;
use std::collections::HashMap;
use uuid::Uuid;

pub trait LedgerStore {
    fn save_account(&mut self, account: Account) -> Result<(), LedgerError>;
    fn get_account(&self, id: Uuid) -> Result<Option<Account>, LedgerError>;
    fn all_accounts(&self) -> Result<Vec<Account>, LedgerError>;

    fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError>;
    fn get_entry(&self, id: Uuid) -> Result<Option<Entry>, LedgerError>;
    fn entries_for_account(&self, account_id: Uuid) -> Result<Vec<Entry>, LedgerError>;

    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<EntryPart>) -> Result<(), LedgerError>;
    fn parts_for_entry(&self, entry_id: Uuid) -> Result<Vec<EntryPart>, LedgerError>;

    fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError>;
    fn get_pot(&self, id: Uuid) -> Result<Option<Pot>, LedgerError>;
    fn all_pots(&self) -> Result<Vec<Pot>, LedgerError>;
    fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError>;
    fn allocations_for_pot(&self, pot_id: Uuid) -> Result<Vec<Allocation>, LedgerError>;

    fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError>;
    fn valuations_for_account(&self, account_id: Uuid) -> Result<Vec<Valuation>, LedgerError>;
}

#[derive(Default)]
pub struct InMemoryStore {
    accounts: HashMap<Uuid, Account>,
    entries: HashMap<Uuid, Entry>,
    entry_parts: HashMap<Uuid, Vec<EntryPart>>,
    pots: HashMap<Uuid, Pot>,
    allocations: HashMap<Uuid, Allocation>,
    valuations: HashMap<Uuid, Vec<Valuation>>,
}

// A HashMap operation cannot fail, so every method here returns Ok. The
// Result in the trait exists for SqliteStore, which does real I/O; this
// implementation just satisfies the same interface.
impl LedgerStore for InMemoryStore {
    fn save_account(&mut self, account: Account) -> Result<(), LedgerError> {
        self.accounts.insert(account.id, account);
        Ok(())
    }

    fn get_account(&self, id: Uuid) -> Result<Option<Account>, LedgerError> {
        Ok(self.accounts.get(&id).cloned())
    }

    fn all_accounts(&self) -> Result<Vec<Account>, LedgerError> {
        Ok(self.accounts.values().cloned().collect())
    }

    fn save_entry(&mut self, entry: Entry) -> Result<(), LedgerError> {
        self.entries.insert(entry.id, entry);
        Ok(())
    }

    fn get_entry(&self, id: Uuid) -> Result<Option<Entry>, LedgerError> {
        Ok(self.entries.get(&id).cloned())
    }

    fn entries_for_account(&self, account_id: Uuid) -> Result<Vec<Entry>, LedgerError> {
        Ok(self
            .entries
            .values()
            .filter(|e| e.account_id == account_id)
            .cloned()
            .collect())
    }

    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<EntryPart>) -> Result<(), LedgerError> {
        self.entry_parts.insert(entry_id, parts);
        Ok(())
    }

    fn parts_for_entry(&self, entry_id: Uuid) -> Result<Vec<EntryPart>, LedgerError> {
        Ok(self.entry_parts.get(&entry_id).cloned().unwrap_or_default())
    }

    fn save_pot(&mut self, pot: Pot) -> Result<(), LedgerError> {
        self.pots.insert(pot.id, pot);
        Ok(())
    }

    fn get_pot(&self, id: Uuid) -> Result<Option<Pot>, LedgerError> {
        Ok(self.pots.get(&id).cloned())
    }

    fn all_pots(&self) -> Result<Vec<Pot>, LedgerError> {
        Ok(self.pots.values().cloned().collect())
    }

    fn save_allocation(&mut self, allocation: Allocation) -> Result<(), LedgerError> {
        self.allocations.insert(allocation.id, allocation);
        Ok(())
    }

    fn allocations_for_pot(&self, pot_id: Uuid) -> Result<Vec<Allocation>, LedgerError> {
        Ok(self
            .allocations
            .values()
            .filter(|a| a.pot_id == pot_id)
            .cloned()
            .collect())
    }

    fn save_valuation(&mut self, valuation: Valuation) -> Result<(), LedgerError> {
        self.valuations.entry(valuation.account_id).or_default().push(valuation);
        Ok(())
    }

    fn valuations_for_account(&self, account_id: Uuid) -> Result<Vec<Valuation>, LedgerError> {
        Ok(self.valuations.get(&account_id).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountKind;
    use crate::currency::Currency;
    use rust_decimal_macros::dec;

    #[test]
    fn saved_account_can_be_read_back_by_id() {
        let mut store = InMemoryStore::default();
        let account = Account::new(
            "Checking",
            Currency::new("EUR").unwrap(),
            AccountKind::Own,
            dec!(0),
        );
        let id = account.id;
        store.save_account(account.clone()).unwrap();
        assert_eq!(store.get_account(id), Ok(Some(account)));
    }

    #[test]
    fn unknown_id_returns_none() {
        let store = InMemoryStore::default();
        assert_eq!(store.get_account(Uuid::new_v4()), Ok(None));
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = InMemoryStore::default();
        let eur = Currency::new("EUR").unwrap();
        store.save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0))).unwrap();
        store.save_account(Account::new("B", eur, AccountKind::Own, dec!(0))).unwrap();
        assert_eq!(store.all_accounts().unwrap().len(), 2);
    }
}
