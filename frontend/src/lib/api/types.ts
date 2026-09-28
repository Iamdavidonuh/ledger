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

// Import feature (docs/superpowers/specs/2026-09-27-import.md). Backend
// doesn't exist yet as of writing this -- these mirror the finalized spec,
// not running code, so field names/casing may need reconciling once the
// API is built. Row kind is a discriminated union on `kind` since a Normal
// and a RevertedCandidate row genuinely carry different fields; accessing
// a RevertedCandidate-only field on a Normal row shouldn't type-check.

export type BankType = 'BankA' | 'BankB';

export interface Import {
	id: string;
	account_id: string;
	currency: string;
	file_name: string;
	uploaded_at: string;
	rows_read: number;
	opening_balance: string;
	closing_balance: string;
	completed: boolean;
}

// GET /imports returns this smaller shape, not the full Import -- the list
// screen doesn't need account_id/currency/opening_balance/closing_balance,
// so the API doesn't send them.
export interface ImportSummary {
	id: string;
	file_name: string;
	uploaded_at: string;
	rows_read: number;
	completed: boolean;
}

interface ImportQueueRowBase {
	id: string;
	import_id: string;
	date: string;
	time: string | null;
	amount: string;
	currency: string;
	bank_state: BankState;
}

export interface NormalQueueRow extends ImportQueueRowBase {
	kind: 'Normal';
	description: string;
	category: string | null;
	suspicious: boolean;
	matched_entry_ids: string[];
	matched_queue_row_ids: string[];
	suggested_category: string | null;
}

export interface RevertedCandidateQueueRow extends ImportQueueRowBase {
	kind: 'RevertedCandidate';
	suggested_entry_id: string | null;
}

export type ImportQueueRow = NormalQueueRow | RevertedCandidateQueueRow;

// PATCH /imports/:id/queue/:row_id returns the bare queue row as stored --
// not the QueueRowView shape above (no suspicious/matched_*/suggested_*
// fields; those are only computed for GET /imports/:id/queue).
export interface PatchedQueueRow {
	id: string;
	import_id: string;
	kind: 'Normal' | 'RevertedCandidate';
	date: string;
	time: string | null;
	amount: string;
	currency: string;
	description: string;
	bank_state: BankState;
	category: string | null;
}

export interface ImportResult {
	import_id: string;
	total_rows: number;
	reverted_candidate_count: number;
	suspicious_count: number;
}

export interface AcceptQueueRowRequest {
	category?: string;
}

export interface AcceptAsTransferRequest {
	other_account_id: string;
	other_amount?: string;
}

export interface UpdateQueueRowCategoryRequest {
	category: string | null;
}

export interface AcceptAllResult {
	accepted: number;
}
