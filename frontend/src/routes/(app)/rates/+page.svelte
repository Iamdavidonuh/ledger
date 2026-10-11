<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type AccountWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';

	interface Rate {
		from: string;
		to: string;
		rate: number;
		fetchedAt: string;
	}

	interface NetWorthRow {
		currency: string;
		localTotal: number;
		converted: number | null;
		rateUnavailable: boolean;
	}

	let rates = $state<Rate[]>([]);
	let loading = $state(true);
	let refreshing = $state(false);
	let error = $state<string | null>(null);
	let accounts = $state<AccountWithBalance[]>([]);

	// Base currency: most common currency among own/investment accounts, or first found
	const baseCurrency = $derived.by(() => {
		const netWorthAccounts = accounts.filter(
			(a) => !a.archived && (a.kind === 'own' || a.kind === 'investment')
		);
		if (netWorthAccounts.length === 0) return 'EUR';
		const counts = new Map<string, number>();
		for (const a of netWorthAccounts) counts.set(a.currency, (counts.get(a.currency) ?? 0) + 1);
		return [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0];
	});

	// Sum balances per currency for net-worth accounts
	const balancesByCurrency = $derived.by(() => {
		const map = new Map<string, number>();
		for (const a of accounts) {
			if (a.archived) continue;
			if (a.kind !== 'own' && a.kind !== 'investment') continue;
			const value = a.kind === 'investment' && a.current_value !== null
				? parseFloat(a.current_value)
				: parseFloat(a.balance);
			map.set(a.currency, (map.get(a.currency) ?? 0) + value);
		}
		return map;
	});

	let netWorthRows = $state<NetWorthRow[]>([]);
	const blendedTotal = $derived(
		netWorthRows.every((r) => r.converted !== null)
			? netWorthRows.reduce((sum, r) => sum + (r.converted ?? 0), 0)
			: null
	);
	const partialTotal = $derived(
		netWorthRows.reduce((sum, r) => sum + (r.converted ?? r.localTotal), 0)
	);
	const anyRateUnavailable = $derived(netWorthRows.some((r) => r.rateUnavailable));

	onMount(async () => {
		await Promise.all([loadAccounts(), loadRates()]);
	});

	async function loadAccounts() {
		try {
			accounts = await api.accounts.list();
		} catch {
			// accounts remain empty — net worth section will not render
		}
	}

	async function loadRates() {
		try {
			loading = true;
			error = null;

			// Collect unique non-base currencies from net-worth accounts
			await loadAccounts(); // ensure accounts loaded before determining pairs
			const currencies = [...new Set(
				accounts
					.filter((a) => !a.archived && (a.kind === 'own' || a.kind === 'investment'))
					.map((a) => a.currency)
			)];

			const base = baseCurrency;
			const nonBase = currencies.filter((c) => c !== base);

			// Fetch one rate per foreign currency (foreign → base), tolerating failures
			const rateResults = await Promise.allSettled(
				nonBase.map((c) => api.rates.get(c, base))
			);

			const rateMap = new Map<string, number>();
			const fetchedRates: Rate[] = [];

			for (let i = 0; i < nonBase.length; i++) {
				const result = rateResults[i];
				if (result.status === 'fulfilled') {
					const r = result.value;
					const rateNum = parseFloat(r.rate);
					rateMap.set(nonBase[i], rateNum);
					fetchedRates.push({
						from: r.from,
						to: r.to,
						rate: rateNum,
						fetchedAt: new Date(r.fetched_at).toLocaleString('en-GB', {
							month: 'short',
							day: 'numeric',
							hour: '2-digit',
							minute: '2-digit'
						})
					});
				}
			}

			rates = fetchedRates;

			// Build net worth rows
			netWorthRows = [...balancesByCurrency.entries()].map(([currency, localTotal]) => {
				if (currency === base) {
					return { currency, localTotal, converted: localTotal, rateUnavailable: false };
				}
				const rate = rateMap.get(currency);
				if (rate === undefined) {
					return { currency, localTotal, converted: null, rateUnavailable: true };
				}
				return { currency, localTotal, converted: localTotal * rate, rateUnavailable: false };
			});
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load rates';
		} finally {
			loading = false;
		}
	}

	async function refresh() {
		try {
			refreshing = true;
			error = null;
			await api.rates.refresh();
			await loadRates();
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to refresh rates';
		} finally {
			refreshing = false;
		}
	}

	function formatRate(rate: Rate): string {
		return `1 ${rate.from} = ${rate.rate < 0.01 ? rate.rate.toFixed(6) : rate.rate.toFixed(4)} ${rate.to}`;
	}
</script>

<div class="flex flex-col gap-6 lg:max-w-2xl">
	<div class="font-display text-xl font-bold">Exchange rates</div>

	<!-- Blended net worth -->
	{#if netWorthRows.length > 0}
		<Card>
			<CardContent class="pt-5">
				<div class="text-xs font-semibold uppercase tracking-wide text-muted">
					Total net worth ({baseCurrency}){anyRateUnavailable ? ' · Partial' : ''}
				</div>
				<div class="mt-1 font-display text-4xl font-bold">
					{formatMoney(blendedTotal ?? partialTotal, baseCurrency)}
				</div>
				<div class="mt-3 flex flex-col gap-1.5">
					{#each netWorthRows as row (row.currency)}
						<div class="flex items-baseline justify-between text-sm">
							<span class="text-muted">{row.currency}</span>
							<span class="text-muted-2">
								{formatMoney(row.localTotal, row.currency)}
								{#if row.currency !== baseCurrency}
									{#if row.rateUnavailable}
										<span class="ml-1 text-xs text-muted">(rate unavailable)</span>
									{:else}
										<span class="text-muted-2"> → </span>
										<span class="font-display font-semibold text-foreground">{formatMoney(row.converted!, baseCurrency)}</span>
									{/if}
								{/if}
							</span>
						</div>
					{/each}
				</div>
			</CardContent>
		</Card>
	{:else if !loading}
		<Card>
			<CardContent class="pt-5 text-sm text-muted">
				No net-worth accounts found. Add an account of type Own or Investment to see totals here.
			</CardContent>
		</Card>
	{/if}

	<!-- Rates list -->
	<div class="flex flex-col gap-3">
		<div class="flex items-baseline justify-between">
			<div class="text-xs font-semibold uppercase tracking-wide text-muted">Current rates</div>
			<Button size="sm" variant="ghost" disabled={loading || refreshing} onclick={refresh}>
				{refreshing ? 'Refreshing...' : 'Refresh'}
			</Button>
		</div>

		{#if loading}
			<p class="text-sm text-muted">Loading rates...</p>
		{:else if error}
			<Card class="border-warn-border bg-warn-bg">
				<CardContent class="pt-4 text-sm text-warn-foreground">{error}</CardContent>
			</Card>
		{:else if rates.length === 0}
			<p class="text-sm text-muted">No rates fetched yet. Add accounts in multiple currencies to see rates.</p>
		{:else}
			{#each rates as rate (`${rate.from}-${rate.to}`)}
				<Card>
					<CardContent class="flex items-center justify-between pt-4">
						<div>
							<div class="font-display font-semibold">{formatRate(rate)}</div>
							<div class="mt-0.5 text-xs text-muted">Fetched {rate.fetchedAt}</div>
						</div>
						<div class="rounded-full bg-page px-2.5 py-1 text-xs font-semibold text-muted">
							{rate.from}/{rate.to}
						</div>
					</CardContent>
				</Card>
			{/each}
		{/if}

		<Card class="border-primary-soft bg-primary-soft/30">
			<CardContent class="pt-4 text-sm text-primary">
				Rates are fetched live from Frankfurter (fiat) and CoinGecko (crypto), cached for 6 hours.
			</CardContent>
		</Card>
	</div>
</div>
