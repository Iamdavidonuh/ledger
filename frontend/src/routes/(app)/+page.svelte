<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type AccountWithBalance, type PotWithBalance } from '$lib/api';
	import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';
	import { Progress } from '$lib/components/ui/progress';
	import { formatMoney } from '$lib/format';

	let accounts = $state<AccountWithBalance[]>([]);
	let pots = $state<PotWithBalance[]>([]);
	let monthIn = $state<Record<string, number>>({});
	let monthOut = $state<Record<string, number>>({});
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Grouped by currency rather than blended into one number: summing
	// balances across currencies without a conversion rate would silently
	// produce a wrong total for anyone with more than one currency.
	const netWorthByCurrency = $derived.by(() => {
		const totals = new Map<string, number>();
		for (const account of accounts) {
			if (account.kind !== 'own') continue;
			totals.set(account.currency, (totals.get(account.currency) ?? 0) + Number(account.balance));
		}
		return totals;
	});

	const potsByCurrency = $derived.by(() => {
		const groups = new Map<string, PotWithBalance[]>();
		for (const pot of pots) {
			const list = groups.get(pot.currency) ?? [];
			list.push(pot);
			groups.set(pot.currency, list);
		}
		return groups;
	});

	function generalSavings(currency: string): number {
		const potsTotal = (potsByCurrency.get(currency) ?? []).reduce((sum, pot) => sum + Number(pot.balance), 0);
		return (netWorthByCurrency.get(currency) ?? 0) - potsTotal;
	}

	onMount(load);

	async function load() {
		loading = true;
		error = null;
		try {
			const [accountList, potList] = await Promise.all([api.accounts.list(), api.pots.list()]);
			accounts = accountList;
			pots = potList;

			const startOfMonth = new Date();
			startOfMonth.setDate(1);
			startOfMonth.setHours(0, 0, 0, 0);

			const inTotals: Record<string, number> = {};
			const outTotals: Record<string, number> = {};
			const perAccountEntries = await Promise.all(accountList.map((account) => api.entries.list(account.id)));
			for (const entries of perAccountEntries) {
				for (const entry of entries) {
					// Voided entries don't count, and a transfer is neither
					// income nor spending, just money changing accounts.
					if (entry.voided_reason !== null) continue;
					if (entry.transfer_account_id !== null) continue;
					if (new Date(entry.date) < startOfMonth) continue;
					const amount = Number(entry.amount);
					if (amount >= 0) {
						inTotals[entry.currency] = (inTotals[entry.currency] ?? 0) + amount;
					} else {
						outTotals[entry.currency] = (outTotals[entry.currency] ?? 0) - amount;
					}
				}
			}
			monthIn = inTotals;
			monthOut = outTotals;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your accounts.';
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex flex-col gap-4">
	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else}
		{#each [...netWorthByCurrency.entries()] as [currency, total] (currency)}
			<Card>
				<CardHeader>
					<CardTitle>Net worth{netWorthByCurrency.size > 1 ? ` (${currency})` : ''}</CardTitle>
					<div class="font-display text-4xl font-bold">{formatMoney(total, currency)}</div>
				</CardHeader>
				<CardContent>
					<div class="flex flex-wrap gap-2">
						{#each accounts.filter((a) => a.kind === 'own' && a.currency === currency) as account (account.id)}
							<div class="flex items-center gap-1.5 rounded-lg bg-page px-2.5 py-1.5 text-xs">
								<span class="text-muted">{account.name}</span>
								<span class="font-display font-semibold">{formatMoney(Number(account.balance), currency)}</span>
							</div>
						{/each}
					</div>
				</CardContent>
			</Card>
		{/each}

		{#if pots.length > 0}
			<div class="flex flex-col gap-2.5">
				<div class="flex items-baseline justify-between">
					<div class="text-xs font-semibold tracking-wide text-muted uppercase">Pots</div>
					<a href="/pots" class="text-sm font-semibold text-primary">See all</a>
				</div>
				{#each pots as pot (pot.id)}
					<a href="/pots" class="block rounded-2xl border border-border bg-card p-4 hover:bg-row-hover">
						<div class="mb-2 flex items-baseline justify-between">
							<div class="text-sm font-semibold">{pot.name}</div>
							<div class="font-display text-xs text-muted">
								{formatMoney(Number(pot.balance), pot.currency)}{#if pot.target}
									/ {formatMoney(Number(pot.target), pot.currency)}{/if}
							</div>
						</div>
						{#if pot.target}
							<Progress value={Number(pot.balance)} max={Number(pot.target)} />
						{/if}
					</a>
				{/each}
				{#each [...netWorthByCurrency.keys()] as currency (currency)}
					<div class="flex items-center justify-between rounded-2xl border border-dashed border-border-dashed bg-card p-4">
						<div>
							<div class="text-sm font-semibold text-muted">General savings</div>
							<div class="text-xs text-muted-2">Not set aside for anything</div>
						</div>
						<div class="font-display text-[15px] font-bold">{formatMoney(generalSavings(currency), currency)}</div>
					</div>
				{/each}
			</div>
		{/if}

		<div class="flex flex-col gap-2.5">
			<div class="text-xs font-semibold tracking-wide text-muted uppercase">This month</div>
			<div class="flex gap-2.5">
				{#each Object.keys({ ...monthIn, ...monthOut }) as currency (currency)}
					<div class="flex flex-1 flex-col gap-1 rounded-2xl border border-border bg-card p-3.5">
						<div class="text-xs text-muted">In ({currency})</div>
						<div class="font-display text-lg font-bold text-primary">
							+{formatMoney(monthIn[currency] ?? 0, currency)}
						</div>
					</div>
					<div class="flex flex-1 flex-col gap-1 rounded-2xl border border-border bg-card p-3.5">
						<div class="text-xs text-muted">Out ({currency})</div>
						<div class="font-display text-lg font-bold">&minus;{formatMoney(monthOut[currency] ?? 0, currency)}</div>
					</div>
				{/each}
			</div>
		</div>
	{/if}
</div>
