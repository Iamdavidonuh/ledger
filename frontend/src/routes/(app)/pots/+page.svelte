<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type PotWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Progress } from '$lib/components/ui/progress';
	import { formatMoney } from '$lib/format';

	let pots = $state<PotWithBalance[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	onMount(async () => {
		try {
			pots = await api.pots.list();
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your pots.';
		} finally {
			loading = false;
		}
	});
</script>

<div class="flex flex-col gap-4">
	<div class="font-display text-xl font-bold">Pots</div>

	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else if pots.length === 0}
		<p class="text-sm text-muted">No pots yet.</p>
	{:else}
		<div class="flex flex-col gap-2.5 lg:grid lg:grid-cols-2 lg:gap-4 xl:grid-cols-3">
			{#each pots as pot (pot.id)}
				<Card>
					<CardContent class="pt-4">
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
					</CardContent>
				</Card>
			{/each}
		</div>
	{/if}
</div>
