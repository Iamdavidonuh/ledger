import type {
	Account,
	AccountWithBalance,
	AcceptAllResult,
	AcceptAsTransferRequest,
	AcceptQueueRowRequest,
	AllocateRequest,
	BankType,
	CreateAccountRequest,
	EditAmountRequest,
	Entry,
	Import,
	ImportQueueRow,
	ImportResult,
	ImportSummary,
	OpenPotRequest,
	PatchedQueueRow,
	Pot,
	PotWithBalance,
	RecordEntryRequest,
	TransferRequest,
	TransferResponse,
	UpdateEntryMetadataRequest,
	UpdateQueueRowCategoryRequest,
	UpdateValueRequest,
	Valuation,
	VoidRequest
} from './types';

// A runtime value, not a build-time constant: this app is a static SPA
// with no server process to inject config into it at deploy time, so the
// deployed API's address comes from /config.js instead, a plain static
// file a deployment can overwrite before this bundle ever runs (see
// app.html and app.d.ts). Locally, config.js's default keeps this at
// '/api', which the dev server proxies to the Axum API (vite.config.ts).
const BASE = window.__ENV__?.API_BASE_URL ?? '/api';

export class ApiError extends Error {
	status: number;
	// The full parsed error body, beyond just its `error` string -- some
	// endpoints attach machine-readable fields here (e.g. accept-all's 409
	// carries suspicious_count/reverted_candidate_count, a failed balance
	// check's 422 carries expected/actual), which callers can read off this.
	body: Record<string, unknown>;

	constructor(status: number, message: string, body: Record<string, unknown> = {}) {
		super(message);
		this.name = 'ApiError';
		this.status = status;
		this.body = body;
	}
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	const response = await fetch(`${BASE}${path}`, {
		...init,
		headers: { 'content-type': 'application/json', ...init?.headers }
	});
	if (!response.ok) {
		const body = await response.json().catch(() => ({ error: response.statusText }));
		throw new ApiError(response.status, body.error ?? response.statusText, body);
	}
	if (response.status === 204) {
		return undefined as T;
	}
	return response.json() as Promise<T>;
}

function get<T>(path: string): Promise<T> {
	return request<T>(path);
}

function post<T>(path: string, body?: unknown): Promise<T> {
	return request<T>(path, { method: 'POST', body: body !== undefined ? JSON.stringify(body) : undefined });
}

function patch<T>(path: string, body: unknown): Promise<T> {
	return request<T>(path, { method: 'PATCH', body: JSON.stringify(body) });
}

function del<T>(path: string): Promise<T> {
	return request<T>(path, { method: 'DELETE' });
}

// Multipart upload: no content-type header set manually, so the browser
// fills in the multipart boundary itself. request() always sets JSON's
// content-type, so this bypasses it rather than reusing it.
async function postForm<T>(path: string, form: FormData): Promise<T> {
	const response = await fetch(`${BASE}${path}`, { method: 'POST', body: form });
	if (!response.ok) {
		const body = await response.json().catch(() => ({ error: response.statusText }));
		throw new ApiError(response.status, body.error ?? response.statusText, body);
	}
	return response.json() as Promise<T>;
}

export const api = {
	accounts: {
		list: () => get<AccountWithBalance[]>('/accounts'),
		get: (id: string) => get<AccountWithBalance>(`/accounts/${id}`),
		create: (req: CreateAccountRequest) => post<Account>('/accounts', req),
		updateCurrentValue: (id: string, req: UpdateValueRequest) =>
			post<Valuation>(`/accounts/${id}/current-value`, req)
	},
	entries: {
		list: (accountId: string) => get<Entry[]>(`/entries?account_id=${accountId}`),
		// GET /entries has no "every account" mode, so this is the fan-out
		// Home and Stats both need to compute a totals across the ledger.
		listForAccounts: async (accountIds: string[]): Promise<Entry[]> => {
			const perAccount = await Promise.all(accountIds.map((id) => get<Entry[]>(`/entries?account_id=${id}`)));
			return perAccount.flat();
		},
		record: (req: RecordEntryRequest) => post<Entry>('/entries', req),
		updateMetadata: (id: string, req: UpdateEntryMetadataRequest) => patch<Entry>(`/entries/${id}`, req),
		editAmount: (id: string, req: EditAmountRequest) => patch<Entry>(`/entries/${id}/amount`, req),
		void: (id: string, req: VoidRequest) => post<Entry>(`/entries/${id}/void`, req),
		confirm: (id: string) => post<Entry>(`/entries/${id}/confirm`)
	},
	pots: {
		list: () => get<PotWithBalance[]>('/pots'),
		get: (id: string) => get<PotWithBalance>(`/pots/${id}`),
		open: (req: OpenPotRequest) => post<Pot>('/pots', req),
		allocate: (id: string, req: AllocateRequest) => post<PotWithBalance>(`/pots/${id}/allocations`, req),
		delete: (id: string) => del<void>(`/pots/${id}`)
	},
	transfers: {
		create: (req: TransferRequest) => post<TransferResponse>('/transfers', req)
	},
	imports: {
		list: () => get<ImportSummary[]>('/imports'),
		get: (id: string) => get<Import>(`/imports/${id}`),
		upload: (accountId: string, bankType: BankType, file: File) => {
			const form = new FormData();
			form.append('account_id', accountId);
			form.append('bank_type', bankType);
			form.append('file', file);
			return postForm<ImportResult>('/imports', form);
		},
		queue: (id: string) => get<ImportQueueRow[]>(`/imports/${id}/queue`),
		acceptAll: (id: string) => post<AcceptAllResult>(`/imports/${id}/queue/accept-all`),
		discardAll: (id: string) => post<{ discarded: number }>(`/imports/${id}/queue/discard-all`),
		accept: (id: string, rowId: string, req?: AcceptQueueRowRequest) =>
			post<Entry>(`/imports/${id}/queue/${rowId}/accept`, req ?? {}),
		acceptAsTransfer: (id: string, rowId: string, req: AcceptAsTransferRequest) =>
			post<TransferResponse>(`/imports/${id}/queue/${rowId}/accept-as-transfer`, req),
		resolveRevert: (id: string, rowId: string) => post<Entry>(`/imports/${id}/queue/${rowId}/resolve-revert`),
		updateCategory: (id: string, rowId: string, req: UpdateQueueRowCategoryRequest) =>
			patch<PatchedQueueRow>(`/imports/${id}/queue/${rowId}`, req),
		discard: (id: string, rowId: string) => post<void>(`/imports/${id}/queue/${rowId}/discard`)
	}
};
