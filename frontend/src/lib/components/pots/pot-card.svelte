<script lang="ts">
	import type { PotWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { Progress } from '$lib/components/ui/progress';
	import { formatMoney } from '$lib/format';
	import AllocateForm from './allocate-form.svelte';

	let {
		pot,
		onallocated
	}: {
		pot: PotWithBalance;
		onallocated: (updated: PotWithBalance) => void;
	} = $props();

	let open = $state(false);
</script>

<Card>
	<CardContent class="flex flex-col gap-3 pt-4">
		<div>
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
		</div>

		{#if open}
			<AllocateForm
				{pot}
				onallocated={(updated) => {
					open = false;
					onallocated(updated);
				}}
				oncancel={() => (open = false)}
			/>
		{:else}
			<Button variant="outline" size="sm" class="self-start max-lg:h-11" onclick={() => (open = true)}>
				Add money
			</Button>
		{/if}
	</CardContent>
</Card>
