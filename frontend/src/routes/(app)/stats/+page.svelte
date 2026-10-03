<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type AccountWithBalance, type Entry } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Progress } from '$lib/components/ui/progress';
	import { formatMoney } from '$lib/format';
	import { parseLocalDate, startOfCurrentMonth } from '$lib/dates';

	type CategoryTotal = { category: string; amount: number };

	let loading = $state(true);
	let error = $state<string | null>(null);
	let entries = $state<Entry[]>([]);

	// Local date components, not toISOString() (UTC) -- the same local-vs-UTC
	// mismatch this page's own spending calculation was just fixed for.
	function toDateInput(date: Date): string {
		const year = date.getFullYear();
		const month = (date.getMonth() + 1).toString().padStart(2, '0');
		const day = date.getDate().toString().padStart(2, '0');
		return `${year}-${month}-${day}`;
	}

	let from = $state(toDateInput(startOfCurrentMonth()));
	let to = $state(toDateInput(new Date()));

	function resetToThisMonth() {
		from = toDateInput(startOfCurrentMonth());
		to = toDateInput(new Date());
	}

	// category -> currency -> total, for entries dated within [from, to].
	// Only expenses count toward spending; income and transfers aren't
	// "spending by category". Entirely client-side: the entries API has no
	// date filter of its own, and an account's full history loaded once is
	// small enough that re-deriving this per range change is instant.
	const spendingByCurrency = $derived.by(() => {
		const start = parseLocalDate(from);
		const end = parseLocalDate(to);
		const totals = new Map<string, Map<string, number>>();
		for (const entry of entries) {
			if (entry.voided_reason !== null) continue;
			if (entry.transfer_account_id !== null) continue;
			const entryDate = parseLocalDate(entry.date);
			if (entryDate < start || entryDate > end) continue;
			const amount = Number(entry.amount);
			if (amount >= 0) continue;

			const byCurrency = totals.get(entry.currency) ?? new Map<string, number>();
			const category = entry.category ?? 'Uncategorized';
			byCurrency.set(category, (byCurrency.get(category) ?? 0) - amount);
			totals.set(entry.currency, byCurrency);
		}

		const result = new Map<string, CategoryTotal[]>();
		for (const [currency, byCategory] of totals) {
			const sorted = [...byCategory.entries()]
				.map(([category, amount]) => ({ category, amount }))
				.sort((a, b) => b.amount - a.amount);
			result.set(currency, sorted);
		}
		return result;
	});

	onMount(async () => {
		try {
			const accounts = await api.accounts.list();
			entries = await api.entries.listForAccounts(accounts.map((a: AccountWithBalance) => a.id));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your stats.';
		} finally {
			loading = false;
		}
	});
</script>

<div class="flex flex-col gap-4 lg:max-w-2xl">
	<div class="font-display text-xl font-bold">Stats</div>

	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else}
		<div class="flex flex-wrap items-end gap-2 @container">
			<div class="flex flex-col gap-1">
				<label for="from" class="text-xs font-semibold text-muted">From</label>
				<input
					id="from"
					type="date"
					class="min-h-11 rounded-lg border border-border bg-card px-2.5 text-sm"
					bind:value={from}
					max={to}
				/>
			</div>
			<div class="flex flex-col gap-1">
				<label for="to" class="text-xs font-semibold text-muted">To</label>
				<input
					id="to"
					type="date"
					class="min-h-11 rounded-lg border border-border bg-card px-2.5 text-sm"
					bind:value={to}
					min={from}
				/>
			</div>
			<button type="button" class="min-h-11 px-1 text-sm font-semibold text-primary" onclick={resetToThisMonth}>
				This month
			</button>
		</div>

		{#if spendingByCurrency.size === 0}
			<p class="text-sm text-muted">No spending recorded in this range.</p>
		{:else}
			{#each [...spendingByCurrency.entries()] as [currency, categories] (currency)}
				<div class="flex flex-col gap-2.5">
					<div class="text-xs font-semibold tracking-wide text-muted uppercase">
						Spending by category ({currency})
					</div>
					{#each categories as { category, amount } (category)}
						<Card>
							<CardContent class="flex flex-col gap-2 pt-4">
								<div class="flex items-baseline justify-between">
									<div class="text-sm font-semibold">{category}</div>
									<div class="font-display text-sm font-bold">{formatMoney(amount, currency)}</div>
								</div>
								<Progress value={amount} max={categories[0].amount} />
							</CardContent>
						</Card>
					{/each}
				</div>
			{/each}
		{/if}
	{/if}
</div>
