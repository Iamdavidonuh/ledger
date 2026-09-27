use crate::account::Account;
use crate::entry::{Entry, EntryPart};
use crate::pot::{Allocation, Pot};
use crate::valuation::Valuation;
use std::collections::HashMap;
use uuid::Uuid;

pub trait LedgerStore {
    fn save_account(&mut self, account: Account);
    fn get_account(&self, id: Uuid) -> Option<Account>;
    fn all_accounts(&self) -> Vec<Account>;

    fn save_entry(&mut self, entry: Entry);
    fn get_entry(&self, id: Uuid) -> Option<Entry>;
    fn entries_for_account(&self, account_id: Uuid) -> Vec<Entry>;

    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<EntryPart>);
    fn parts_for_entry(&self, entry_id: Uuid) -> Vec<EntryPart>;

    fn save_pot(&mut self, pot: Pot);
    fn get_pot(&self, id: Uuid) -> Option<Pot>;
    fn all_pots(&self) -> Vec<Pot>;
    fn save_allocation(&mut self, allocation: Allocation);
    fn allocations_for_pot(&self, pot_id: Uuid) -> Vec<Allocation>;

    fn save_valuation(&mut self, valuation: Valuation);
    fn valuations_for_account(&self, account_id: Uuid) -> Vec<Valuation>;
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

impl LedgerStore for InMemoryStore {
    fn save_account(&mut self, account: Account) {
        self.accounts.insert(account.id, account);
    }

    fn get_account(&self, id: Uuid) -> Option<Account> {
        self.accounts.get(&id).cloned()
    }

    fn all_accounts(&self) -> Vec<Account> {
        self.accounts.values().cloned().collect()
    }

    fn save_entry(&mut self, entry: Entry) {
        self.entries.insert(entry.id, entry);
    }

    fn get_entry(&self, id: Uuid) -> Option<Entry> {
        self.entries.get(&id).cloned()
    }

    fn entries_for_account(&self, account_id: Uuid) -> Vec<Entry> {
        self.entries
            .values()
            .filter(|e| e.account_id == account_id)
            .cloned()
            .collect()
    }

    fn save_entry_parts(&mut self, entry_id: Uuid, parts: Vec<EntryPart>) {
        self.entry_parts.insert(entry_id, parts);
    }

    fn parts_for_entry(&self, entry_id: Uuid) -> Vec<EntryPart> {
        self.entry_parts.get(&entry_id).cloned().unwrap_or_default()
    }

    fn save_pot(&mut self, pot: Pot) {
        self.pots.insert(pot.id, pot);
    }

    fn get_pot(&self, id: Uuid) -> Option<Pot> {
        self.pots.get(&id).cloned()
    }

    fn all_pots(&self) -> Vec<Pot> {
        self.pots.values().cloned().collect()
    }

    fn save_allocation(&mut self, allocation: Allocation) {
        self.allocations.insert(allocation.id, allocation);
    }

    fn allocations_for_pot(&self, pot_id: Uuid) -> Vec<Allocation> {
        self.allocations
            .values()
            .filter(|a| a.pot_id == pot_id)
            .cloned()
            .collect()
    }

    fn save_valuation(&mut self, valuation: Valuation) {
        self.valuations
            .entry(valuation.account_id)
            .or_default()
            .push(valuation);
    }

    fn valuations_for_account(&self, account_id: Uuid) -> Vec<Valuation> {
        self.valuations.get(&account_id).cloned().unwrap_or_default()
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
        store.save_account(account.clone());
        assert_eq!(store.get_account(id), Some(account));
    }

    #[test]
    fn unknown_id_returns_none() {
        let store = InMemoryStore::default();
        assert_eq!(store.get_account(Uuid::new_v4()), None);
    }

    #[test]
    fn all_accounts_lists_every_saved_account() {
        let mut store = InMemoryStore::default();
        let eur = Currency::new("EUR").unwrap();
        store.save_account(Account::new("A", eur.clone(), AccountKind::Own, dec!(0)));
        store.save_account(Account::new("B", eur, AccountKind::Own, dec!(0)));
        assert_eq!(store.all_accounts().len(), 2);
    }
}
