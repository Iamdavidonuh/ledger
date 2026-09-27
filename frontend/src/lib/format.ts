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
