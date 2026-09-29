<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type AccountWithBalance, type Pot, type PotWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';
	import PotCard from '$lib/components/pots/pot-card.svelte';
	import NewPotForm from '$lib/components/pots/new-pot-form.svelte';

	let accounts = $state<AccountWithBalance[]>([]);
	let pots = $state<PotWithBalance[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let creating = $state(false);

	// General savings, per currency: the part of your own accounts' balance
	// no pot has claimed yet. Mirrors the backend's own definition exactly
	// (Ledger::general_savings) so this reads the same number allocating
	// into a pot checks against.
	const savingsCurrencies = $derived(
		[...new Set(accounts.filter((a) => a.kind === 'own' && !a.archived).map((a) => a.currency))].sort()
	);
	const generalSavings = $derived.by(() => {
		const ownTotals = new Map<string, number>();
		for (const account of accounts) {
			if (account.kind !== 'own') continue;
			ownTotals.set(account.currency, (ownTotals.get(account.currency) ?? 0) + Number(account.balance));
		}
		const potTotals = new Map<string, number>();
		for (const pot of pots) {
			potTotals.set(pot.currency, (potTotals.get(pot.currency) ?? 0) + Number(pot.balance));
		}
		return savingsCurrencies.map((currency) => ({
			currency,
			amount: (ownTotals.get(currency) ?? 0) - (potTotals.get(currency) ?? 0)
		}));
	});

	onMount(load);

	async function load() {
		loading = true;
		error = null;
		try {
			[accounts, pots] = await Promise.all([api.accounts.list(), api.pots.list()]);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your pots.';
		} finally {
			loading = false;
		}
	}

	function replacePot(updated: PotWithBalance) {
		pots = pots.map((pot) => (pot.id === updated.id ? updated : pot));
	}

	function addPot(pot: Pot) {
		creating = false;
		pots = [...pots, { ...pot, balance: '0' }];
	}
</script>

<div class="flex flex-col gap-4">
	<div class="flex items-center justify-between gap-3">
		<div class="font-display text-xl font-bold">Pots</div>
		{#if !loading && !error && savingsCurrencies.length > 0}
			<Button size="sm" class="max-lg:h-11" onclick={() => (creating = !creating)}>
				{creating ? 'Cancel' : 'New pot'}
			</Button>
		{/if}
	</div>

	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else if savingsCurrencies.length === 0}
		<p class="text-sm text-muted">Open an account first.</p>
	{:else}
		{#if generalSavings.length > 0}
			<Card class="bg-primary-soft">
				<CardContent class="flex flex-wrap gap-x-6 gap-y-1 pt-4">
					{#each generalSavings as entry (entry.currency)}
						<div>
							<div class="text-xs font-semibold text-muted">General savings ({entry.currency})</div>
							<div class="font-display text-base font-bold">{formatMoney(entry.amount, entry.currency)}</div>
						</div>
					{/each}
				</CardContent>
			</Card>
		{/if}

		{#if creating}
			<NewPotForm currencies={savingsCurrencies} oncreated={addPot} oncancel={() => (creating = false)} />
		{/if}

		{#if pots.length === 0}
			<p class="text-sm text-muted">No pots yet.</p>
		{:else}
			<div class="flex flex-col gap-2.5 lg:grid lg:grid-cols-2 lg:gap-4 xl:grid-cols-3">
				{#each pots as pot (pot.id)}
					<PotCard {pot} onallocated={replacePot} />
				{/each}
			</div>
		{/if}
	{/if}
</div>
