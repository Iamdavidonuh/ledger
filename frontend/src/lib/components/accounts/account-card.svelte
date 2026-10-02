<script lang="ts">
	import { api, ApiError, type AccountWithBalance, type Valuation } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';
	import UpdateValueForm from './update-value-form.svelte';

	let {
		account,
		onchanged
	}: {
		account: AccountWithBalance;
		onchanged: (updated: AccountWithBalance) => void;
	} = $props();

	let mode = $state<'idle' | 'value'>('idle');
	let archiving = $state(false);
	let error = $state<string | null>(null);

	// An Investment account's real worth is its own current_value once set
	// (entries don't drive it day to day); everything else just shows its
	// entry-derived balance. Matches Ledger::current_value's own fallback.
	const displayAmount = $derived(account.current_value ?? account.balance);

	async function toggleArchived() {
		archiving = true;
		error = null;
		try {
			const updated = account.archived
				? await api.accounts.unarchive(account.id)
				: await api.accounts.archive(account.id);
			onchanged({ ...account, ...updated });
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not update this account.';
		} finally {
			archiving = false;
		}
	}

	function valueUpdated(valuation: Valuation) {
		mode = 'idle';
		onchanged({ ...account, current_value: valuation.new_value });
	}
</script>

<Card class={account.archived ? 'opacity-60' : ''}>
	<CardContent class="flex flex-col gap-3 pt-5">
		<div class="flex items-center justify-between">
			<div>
				<div class="text-sm font-semibold">{account.name}</div>
				<div class="text-xs text-muted capitalize">
					{account.kind} &middot; {account.currency}{#if account.archived}&nbsp;&middot; archived{/if}
				</div>
			</div>
			<div class="font-display text-base font-bold">{formatMoney(Number(displayAmount), account.currency)}</div>
		</div>

		{#if mode === 'value'}
			<UpdateValueForm {account} onupdated={valueUpdated} oncancel={() => (mode = 'idle')} />
		{:else}
			{#if error}
				<p class="text-xs text-warn-foreground">{error}</p>
			{/if}
			<div class="flex flex-wrap gap-3">
				{#if account.kind === 'investment' && !account.archived}
					<Button variant="outline" size="sm" class="max-lg:h-11" onclick={() => (mode = 'value')}>
						Update value
					</Button>
				{/if}
				<Button
					type="button"
					variant="ghost"
					size="sm"
					class="max-lg:h-11 text-muted"
					disabled={archiving}
					onclick={toggleArchived}
				>
					{archiving ? 'Saving...' : account.archived ? 'Unarchive' : 'Archive'}
				</Button>
			</div>
		{/if}
	</CardContent>
</Card>
