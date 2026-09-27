import type {
	Account,
	AccountWithBalance,
	AllocateRequest,
	CreateAccountRequest,
	EditAmountRequest,
	Entry,
	OpenPotRequest,
	Pot,
	PotWithBalance,
	RecordEntryRequest,
	TransferRequest,
	TransferResponse,
	UpdateEntryMetadataRequest,
	UpdateValueRequest,
	Valuation,
	VoidRequest
} from './types';

// Same-origin '/api' prefix: the dev server proxies it to the Axum API
// (vite.config.ts), and in production the ingress routes it to the API
// container the same way, so this client never needs an absolute URL or
// CORS on the API.
const BASE = '/api';

export class ApiError extends Error {
	status: number;

	constructor(status: number, message: string) {
		super(message);
		this.name = 'ApiError';
		this.status = status;
	}
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	const response = await fetch(`${BASE}${path}`, {
		...init,
		headers: { 'content-type': 'application/json', ...init?.headers }
	});
	if (!response.ok) {
		const body = await response.json().catch(() => ({ error: response.statusText }));
		throw new ApiError(response.status, body.error ?? response.statusText);
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
		allocate: (id: string, req: AllocateRequest) => post<PotWithBalance>(`/pots/${id}/allocations`, req)
	},
	transfers: {
		create: (req: TransferRequest) => post<TransferResponse>('/transfers', req)
	}
};
