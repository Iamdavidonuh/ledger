<script lang="ts">
	import { untrack } from 'svelte';
	import type { AccountWithBalance, AcceptAsTransferRequest, NormalQueueRow } from '$lib/api';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import { formatClock, formatDay, formatSignedMoney } from '$lib/format';
	import CategoryField from './category-field.svelte';
	import TransferForm from './transfer-form.svelte';

	// A row nothing else in the ledger resembles: one decision, accept it (with
	// a category if it needs one) or drop it. Accepting is also what saves it,
	// so nothing here is applied until the person presses the button.
	let {
		row,
		account,
		accounts,
		categories,
		busy = false,
		onaccept,
		ondiscard,
		onsetcategory,
		ontransfer
	}: {
		row: NormalQueueRow;
		account: AccountWithBalance;
		accounts: AccountWithBalance[];
		categories: string[];
		busy?: boolean;
		onaccept: (category: string | undefined) => void;
		ondiscard: () => void;
		onsetcategory: (category: string | null) => void;
		ontransfer: (request: AcceptAsTransferRequest) => void;
	} = $props();

	// The field starts from the category saved on the row and is the person's
	// from then on, so reading the prop once here is intended.
	let category = $state(untrack(() => row.category ?? ''));
	let transferOpen = $state(false);

	const clock = $derived(formatClock(row.time));
	const incoming = $derived(Number(row.amount) > 0);
</script>

<Card class="flex flex-col gap-3 p-4">
	<div class="flex items-start justify-between gap-3">
		<div class="flex min-w-0 flex-col gap-1">
			<p class="line-clamp-2 text-sm font-semibold break-words" title={row.description}>{row.description}</p>
			<div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-2">
				<span>{formatDay(row.date)}{clock ? `, ${clock}` : ''}</span>
				{#if row.bank_state === 'pending'}
					<Badge variant="warning" class="px-2 py-0.5">Pending</Badge>
				{/if}
			</div>
		</div>
		<p class="font-display text-sm font-bold whitespace-nowrap {incoming ? 'text-primary' : ''}">
			{formatSignedMoney(row.amount, row.currency)}
		</p>
	</div>

	{#if transferOpen}
		<TransferForm
			{row}
			{account}
			{accounts}
			{busy}
			onsubmit={ontransfer}
			oncancel={() => (transferOpen = false)}
		/>
	{:else}
		<CategoryField
			bind:value={category}
			suggestion={row.suggested_category}
			{categories}
			disabled={busy}
			onchange={(next) => {
				if (next !== (row.category ?? '')) onsetcategory(next === '' ? null : next);
			}}
		/>

		<div class="flex flex-wrap gap-3">
			<Button size="sm" class="max-lg:h-11" disabled={busy} onclick={() => onaccept(category.trim() || undefined)}>
				Accept
			</Button>
			<Button size="sm" variant="outline" class="max-lg:h-11" disabled={busy} onclick={() => (transferOpen = true)}>
				Move to another account
			</Button>
			<Button size="sm" variant="ghost" class="max-lg:h-11" disabled={busy} onclick={ondiscard}>Discard</Button>
		</div>
	{/if}
</Card>
