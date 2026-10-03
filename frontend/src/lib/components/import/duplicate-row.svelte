<script lang="ts">
	import { untrack } from 'svelte';
	import type { AccountWithBalance, AcceptAsTransferRequest, Entry, NormalQueueRow } from '$lib/api';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import CategoryField from './category-field.svelte';
	import LineItem from './line-item.svelte';
	import TransferForm from './transfer-form.svelte';

	// A row that looks like something already recorded, or like another
	// pending row in the same file. Both are shown side by side with the new
	// row so the person can tell a real duplicate from a coincidence. Nothing
	// is decided for them: discard, keep, or move it to another account. The
	// side-by-side layout depends on the card's own width (a container query),
	// not the screen's, since the same card sits in a phone column and in a
	// wide desktop column.
	let {
		row,
		account,
		accounts,
		categories,
		matchedEntries,
		matchedRows,
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
		matchedEntries: Entry[];
		matchedRows: NormalQueueRow[];
		busy?: boolean;
		onaccept: (category: string | undefined) => void;
		ondiscard: () => void;
		onsetcategory: (category: string | null) => void;
		ontransfer: (request: AcceptAsTransferRequest) => void;
	} = $props();

	let category = $state(untrack(() => row.category ?? ''));
	let transferOpen = $state(false);
</script>

<Card class="@container flex flex-col gap-3 border-warn-border p-4">
	<div class="flex flex-wrap items-center gap-2">
		<Badge variant="warning">Possible duplicate</Badge>
		<p class="text-xs text-muted-2">Same date and amount</p>
	</div>

	<div class="grid gap-2 @md:grid-cols-2">
		<div class="flex flex-col gap-2">
			{#each matchedEntries as entry (entry.id)}
				<LineItem
					label="Already in your ledger"
					description={entry.description}
					date={entry.date}
					time={entry.time}
					amount={entry.amount}
					currency={entry.currency}
				/>
			{/each}
			{#each matchedRows as sibling (sibling.id)}
				<LineItem
					label="Also in this import"
					description={sibling.description}
					date={sibling.date}
					time={sibling.time}
					amount={sibling.amount}
					currency={sibling.currency}
				/>
			{/each}
		</div>
		<LineItem
			label="New in this import"
			description={row.description}
			date={row.date}
			time={row.time}
			amount={row.amount}
			currency={row.currency}
		/>
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
			<Button size="sm" class="max-lg:h-11" disabled={busy} onclick={ondiscard}>Discard duplicate</Button>
			<Button
				size="sm"
				variant="outline"
				class="max-lg:h-11"
				disabled={busy}
				onclick={() => onaccept(category.trim() || undefined)}
			>
				Keep as a new entry
			</Button>
			<Button size="sm" variant="ghost" class="max-lg:h-11" disabled={busy} onclick={() => (transferOpen = true)}>
				Move to another account
			</Button>
		</div>
	{/if}
</Card>
