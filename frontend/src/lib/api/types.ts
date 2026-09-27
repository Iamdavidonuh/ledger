// Mirrors ledger-core's domain types and ledger-api's request/response
// shapes exactly (core/src/{account,entry,pot,valuation}.rs,
// api/src/{accounts,entries,pots,transfers,valuations}.rs). rust_decimal
// is configured with the serde-with-str feature, so every amount crosses
// the wire as a string, never a JSON number.

export type AccountKind = 'own' | 'outside' | 'person' | 'investment';
export type EntrySource = 'manual' | 'imported';
export type BankState = 'completed' | 'pending' | 'reverted';
export type EntryKind = 'expense' | 'income';

export interface Account {
	id: string;
	name: string;
	currency: string;
	kind: AccountKind;
	opening_balance: string;
	archived: boolean;
	current_value: string | null;
}

export interface AccountWithBalance extends Account {
	balance: string;
}

export interface Entry {
	id: string;
	account_id: string;
	date: string;
	time: string | null;
	amount: string;
	currency: string;
	description: string;
	note: string | null;
	category: string | null;
	tags: string[];
	pot_id: string | null;
	transfer_account_id: string | null;
	source: EntrySource;
	bank_state: BankState;
	confirmed: boolean;
	voided_reason: string | null;
}

export interface Pot {
	id: string;
	name: string;
	currency: string;
	target: string | null;
	priority: number | null;
}

export interface PotWithBalance extends Pot {
	balance: string;
}

export interface Valuation {
	id: string;
	account_id: string;
	date: string;
	old_value: string;
	new_value: string;
	category: string;
}

export interface CreateAccountRequest {
	name: string;
	currency: string;
	kind: AccountKind;
	opening_balance: string;
}

export interface RecordEntryRequest {
	account_id: string;
	date: string;
	kind: EntryKind;
	amount: string;
	description: string;
}

export interface UpdateEntryMetadataRequest {
	category: string | null;
	tags: string[];
	note: string | null;
	pot_id: string | null;
}

export interface EditAmountRequest {
	kind: EntryKind;
	amount: string;
}

export interface VoidRequest {
	reason: string;
}

export interface OpenPotRequest {
	name: string;
	currency: string;
	target: string | null;
	priority: number | null;
}

export interface AllocateRequest {
	amount: string;
	date: string;
}

export interface TransferRequest {
	from_account_id: string;
	to_account_id: string;
	date: string;
	amount_sent: string;
	amount_received: string;
	description: string;
}

export interface TransferResponse {
	out_entry: Entry;
	in_entry: Entry;
}

export interface UpdateValueRequest {
	new_value: string;
	category: string;
	date: string;
}
