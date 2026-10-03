<script lang="ts" module>
	import type { ImportSummary } from '$lib/api';

	export interface OpenImport {
		summary: ImportSummary;
		accountName: string;
	}
</script>

<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import { formatDay } from '$lib/format';
	import SectionHeading from './section-heading.svelte';

	// Reviews still waiting on the person, and the ones already finished. There
	// is deliberately no undo on a finished import: every row was reviewed
	// before it was accepted, and a wrong entry is voided or edited on the
	// Entries screen like any other.
	let {
		openImports,
		pastImports,
		onreview
	}: {
		openImports: OpenImport[];
		pastImports: ImportSummary[];
		onreview: (importId: string) => void;
	} = $props();

	function uploadedOn(summary: ImportSummary): string {
		return formatDay(summary.uploaded_at.slice(0, 10));
	}
</script>

{#if openImports.length > 0}
	<section class="flex flex-col gap-2.5">
		<SectionHeading title="Waiting for your review" count={openImports.length} />
		{#each openImports as item (item.summary.id)}
			<Card class="flex items-center gap-3 border-warn-border bg-warn-bg p-4">
				<div class="min-w-0 flex-1">
					<p class="truncate text-sm font-semibold text-warn-foreground">{item.summary.file_name}</p>
					<p class="text-xs text-warn-icon">{item.accountName}, {uploadedOn(item.summary)}</p>
				</div>
				<Button size="sm" onclick={() => onreview(item.summary.id)}>Review</Button>
			</Card>
		{/each}
	</section>
{/if}

<section class="flex flex-col gap-2.5">
	<SectionHeading title="Past imports" />
	{#if pastImports.length === 0}
		<p class="text-sm text-muted">Finished imports will show up here.</p>
	{:else}
		{#each pastImports as summary (summary.id)}
			<Card class="flex items-center gap-3 p-4">
				<div class="min-w-0 flex-1">
					<p class="truncate text-sm font-semibold">{summary.file_name}</p>
					<p class="text-xs text-muted-2">{uploadedOn(summary)}, {summary.rows_read} rows</p>
				</div>
				<Button href="/entries?account={summary.account_id}" variant="ghost" size="sm">See entries</Button>
			</Card>
		{/each}
	{/if}
</section>
