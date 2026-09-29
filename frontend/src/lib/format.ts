// Currency here can be any 2-10 letter/digit code (core/src/currency.rs
// allows crypto-style codes), which Intl.NumberFormat doesn't always
// recognize, so fall back to a plain number instead of throwing.
export function formatMoney(amount: number, currency: string): string {
	try {
		return new Intl.NumberFormat(undefined, {
			style: 'currency',
			currency,
			currencyDisplay: 'narrowSymbol'
		}).format(amount);
	} catch {
		return `${amount.toFixed(2)} ${currency}`;
	}
}

// Like formatMoney for an amount as the API sends it (a decimal string),
// with an explicit plus on money coming in so direction reads at a glance.
export function formatSignedMoney(amount: string, currency: string): string {
	const value = Number(amount);
	return `${value > 0 ? '+' : ''}${formatMoney(value, currency)}`;
}

// "2026-03-01" as "Mar 1". Parsed as a local date, not UTC, so the day never
// shifts for someone west of Greenwich.
export function formatDay(date: string): string {
	const parsed = new Date(`${date}T00:00:00`);
	if (Number.isNaN(parsed.getTime())) return date;
	return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(parsed);
}

// "23:40:00" as "23:40", or null for an entry the bank gave no time for.
export function formatClock(time: string | null): string | null {
	return time ? time.slice(0, 5) : null;
}
