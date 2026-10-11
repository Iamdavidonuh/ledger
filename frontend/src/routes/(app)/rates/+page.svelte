<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';

	interface Rate {
		from: string;
		to: string;
		rate: number;
		fetchedAt: string;
	}

	let rates = $state<Rate[]>([]);
	let loading = $state(true);
	let refreshing = $state(false);
	let error = $state<string | null>(null);

	// Blended net worth stub — TODO: compute from actual account balances with rates
	const stubNetWorth = { base: 'EUR', total: 12730.44, breakdown: [
		{ currency: 'EUR', local: 11620.77, converted: 11620.77 },
		{ currency: 'NGN', local: 1_940_000, converted: 1109.67 }
	]};

	onMount(async () => {
		await loadRates();
	});

	async function loadRates() {
		try {
			loading = true;
			error = null;
			// For now, fetch specific pairs - TODO: get all pairs from backend
			const eurToNgn = await api.rates.get('EUR', 'NGN');
			const ngnToEur = await api.rates.get('NGN', 'EUR');
			
			rates = [
				{
					from: ngnToEur.from,
					to: ngnToEur.to,
					rate: parseFloat(ngnToEur.rate),
					fetchedAt: new Date(ngnToEur.fetched_at).toLocaleString('en-GB', {
						month: 'short',
						day: 'numeric',
						hour: '2-digit',
						minute: '2-digit'
					})
				},
				{
					from: eurToNgn.from,
					to: eurToNgn.to,
					rate: parseFloat(eurToNgn.rate),
					fetchedAt: new Date(eurToNgn.fetched_at).toLocaleString('en-GB', {
						month: 'short',
						day: 'numeric',
						hour: '2-digit',
						minute: '2-digit'
					})
				}
			];
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

	<!-- Blended net worth (stub) -->
	<Card>
		<CardContent class="pt-5">
			<div class="text-xs font-semibold uppercase tracking-wide text-muted">Total net worth ({stubNetWorth.base})</div>
			<div class="mt-1 font-display text-4xl font-bold">{formatMoney(stubNetWorth.total, stubNetWorth.base)}</div>
			<div class="mt-3 flex flex-col gap-1.5">
				{#each stubNetWorth.breakdown as row (row.currency)}
					<div class="flex items-baseline justify-between text-sm">
						<span class="text-muted">{row.currency}</span>
						<span class="text-muted-2">
							{formatMoney(row.local, row.currency)}
							{#if row.currency !== stubNetWorth.base}
								<span class="text-muted-2"> → </span>
								<span class="font-display font-semibold text-foreground">{formatMoney(row.converted, stubNetWorth.base)}</span>
							{/if}
						</span>
					</div>
				{/each}
			</div>
		</CardContent>
	</Card>

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
