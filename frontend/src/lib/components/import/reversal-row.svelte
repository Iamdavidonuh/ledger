<script lang="ts">
	import type { Entry, RevertedCandidateQueueRow } from '$lib/api';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import LineItem from './line-item.svelte';

	// The bank says it reversed a payment. When the server matched it to one
	// entry, confirming marks that entry reverted; there is no way to point it
	// at a different entry, so a wrong match is discarded and the right entry
	// voided from the Entries screen. With no match, discard is the only
	// action. `suggestedEntry` can be missing while the row still has a
	// suggestion (the entry list failed to include it), which is a different
	// situation from having no suggestion at all.
	let {
		row,
		suggestedEntry,
		busy = false,
		onresolve,
		ondiscard
	}: {
		row: RevertedCandidateQueueRow;
		suggestedEntry: Entry | undefined;
		busy?: boolean;
		onresolve: () => void;
		ondiscard: () => void;
	} = $props();

	const hasSuggestion = $derived(row.suggested_entry_id !== null);
</script>

<Card class="@container flex flex-col gap-3 border-warn-border p-4">
	<div class="flex flex-wrap items-center gap-2">
		<Badge variant="warning">Bank reversal</Badge>
		<p class="text-xs text-muted-2">
			{#if suggestedEntry}
				This looks like it reverses an entry you already have.
			{:else if hasSuggestion}
				A matching entry was found, but it could not be loaded here.
			{:else}
				No entry in your ledger matches this reversal.
			{/if}
		</p>
	</div>

	<div class="grid gap-2 @md:grid-cols-2">
		<LineItem
			label="Reversed by the bank"
			description="Reversal"
			date={row.date}
			time={row.time}
			amount={row.amount}
			currency={row.currency}
		/>
		{#if suggestedEntry}
			<LineItem
				label="Your entry"
				description={suggestedEntry.description}
				date={suggestedEntry.date}
				time={suggestedEntry.time}
				amount={suggestedEntry.amount}
				currency={suggestedEntry.currency}
			/>
		{/if}
	</div>

	<div class="flex flex-wrap gap-3">
		{#if hasSuggestion}
			<Button size="sm" class="max-lg:h-11" disabled={busy} onclick={onresolve}>Mark entry as reverted</Button>
			<Button size="sm" variant="ghost" class="max-lg:h-11" disabled={busy} onclick={ondiscard}>Discard</Button>
		{:else}
			<Button size="sm" variant="outline" class="max-lg:h-11" disabled={busy} onclick={ondiscard}>Discard</Button>
		{/if}
	</div>
</Card>
