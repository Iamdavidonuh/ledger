<script lang="ts">
	import { api, ApiError, type PotWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { Progress } from '$lib/components/ui/progress';
	import { formatMoney } from '$lib/format';
	import AllocateForm from './allocate-form.svelte';

	let {
		pot,
		onallocated,
		ondeleted
	}: {
		pot: PotWithBalance;
		onallocated: (updated: PotWithBalance) => void;
		ondeleted: (id: string) => void;
	} = $props();

	let mode = $state<'idle' | 'allocate' | 'delete'>('idle');
	let deleting = $state(false);
	let deleteError = $state<string | null>(null);

	async function confirmDelete() {
		deleting = true;
		deleteError = null;
		try {
			await api.pots.delete(pot.id);
			ondeleted(pot.id);
		} catch (err) {
			deleteError = err instanceof ApiError ? err.message : 'Could not delete this pot.';
			deleting = false;
		}
	}
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

		{#if mode === 'allocate'}
			<AllocateForm
				{pot}
				onallocated={(updated) => {
					mode = 'idle';
					onallocated(updated);
				}}
				oncancel={() => (mode = 'idle')}
			/>
		{:else if mode === 'delete'}
			<div class="flex flex-col gap-3 rounded-xl border border-warn-border bg-warn-bg p-3">
				<p class="text-sm text-warn-foreground">
					Delete {pot.name}? {formatMoney(Number(pot.balance), pot.currency)} goes back to general savings, and
					any entry tagged to it stays as it is, just untagged. This can't be undone.
				</p>
				{#if deleteError}
					<p class="text-xs text-warn-foreground">{deleteError}</p>
				{/if}
				<div class="flex flex-wrap gap-3">
					<Button variant="outline" size="sm" class="max-lg:h-11" disabled={deleting} onclick={confirmDelete}>
						{deleting ? 'Deleting...' : 'Delete pot'}
					</Button>
					<Button
						type="button"
						variant="ghost"
						size="sm"
						class="max-lg:h-11"
						disabled={deleting}
						onclick={() => (mode = 'idle')}
					>
						Cancel
					</Button>
				</div>
			</div>
		{:else}
			<div class="flex flex-wrap gap-3">
				<Button variant="outline" size="sm" class="self-start max-lg:h-11" onclick={() => (mode = 'allocate')}>
					Add money
				</Button>
				<Button
					type="button"
					variant="ghost"
					size="sm"
					class="self-start max-lg:h-11 text-muted"
					onclick={() => (mode = 'delete')}
				>
					Delete
				</Button>
			</div>
		{/if}
	</CardContent>
</Card>
