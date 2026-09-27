<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type AccountWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Progress } from '$lib/components/ui/progress';
	import { formatMoney } from '$lib/format';
	import { startOfCurrentMonth } from '$lib/dates';

	type CategoryTotal = { category: string; amount: number };

	let loading = $state(true);
	let error = $state<string | null>(null);
	let spendingByCurrency = $state<Map<string, CategoryTotal[]>>(new Map());

	onMount(async () => {
		try {
			const accounts = await api.accounts.list();
			const entries = await api.entries.listForAccounts(accounts.map((a: AccountWithBalance) => a.id));
			const startOfMonth = startOfCurrentMonth();

			// category -> currency -> total. Only expenses count toward
			// spending; income and transfers aren't "spending by category".
			const totals = new Map<string, Map<string, number>>();
			for (const entry of entries) {
				if (entry.voided_reason !== null) continue;
				if (entry.transfer_account_id !== null) continue;
				if (new Date(entry.date) < startOfMonth) continue;
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
			spendingByCurrency = result;
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
	{:else if spendingByCurrency.size === 0}
		<p class="text-sm text-muted">No spending recorded this month.</p>
	{:else}
		{#each [...spendingByCurrency.entries()] as [currency, categories] (currency)}
			<div class="flex flex-col gap-2.5">
				<div class="text-xs font-semibold tracking-wide text-muted uppercase">
					Spending by category ({currency}, this month)
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
</div>
