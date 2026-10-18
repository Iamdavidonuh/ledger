<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { api, ApiError, type Entry } from '$lib/api';
	import CategoryInput from '$lib/components/entries/category-input.svelte';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';

	interface Part {
		id: number;
		amount: string;
		category: string;
		note: string;
	}

	let entry = $state<Entry | null>(null);
	let allEntries = $state<Entry[]>([]);
	const allCategories = $derived(
		[...new Set(allEntries.flatMap((e) => (e.category ? [e.category] : [])))].sort((a, b) =>
			a.localeCompare(b)
		)
	);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let nextId = $state(3);

	let parts = $state<Part[]>([
		{ id: 1, amount: '', category: '', note: '' },
		{ id: 2, amount: '', category: '', note: '' }
	]);

	const entryId = $derived(page.url.searchParams.get('entry') ?? '');
	const accountId = $derived(page.url.searchParams.get('account') ?? '');

	const originalAmount = $derived(entry ? Math.abs(Number(entry.amount)) : 0);
	const partsTotal = $derived(parts.reduce((sum, p) => sum + (Number(p.amount) || 0), 0));
	const currency = $derived(entry?.currency ?? '');
	const matches = $derived(Math.abs(partsTotal - originalAmount) < 0.005);

	onMount(async () => {
		if (!entryId || !accountId) {
			error = 'Missing entry or account parameter.';
			loading = false;
			return;
		}
		try {
			const entries = await api.entries.list(accountId);
			allEntries = entries;
			entry = entries.find((e) => e.id === entryId) ?? null;
			if (!entry) error = 'Entry not found.';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not load this entry.';
		} finally {
			loading = false;
		}
	});

	function addPart() {
		parts = [...parts, { id: nextId++, amount: '', category: '', note: '' }];
	}

	function removePart(id: number) {
		if (parts.length <= 2) return;
		parts = parts.filter((p) => p.id !== id);
	}

	async function submit() {
		if (!matches || busy) return;
		busy = true;
		error = null;
		try {
			const partsPayload = parts.map(p => ({
				amount: p.amount,
				category: p.category || undefined
			}));
			await api.entries.split(entryId, partsPayload);
			await goto(`/entries?account=${accountId}`);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not save split.';
			busy = false;
		}
	}
</script>

<div class="flex flex-col gap-4 lg:max-w-2xl">
	<div class="flex items-center gap-3">
		<Button variant="ghost" size="icon" href="/entries?account={accountId}" aria-label="Back to entries">
			<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M15 6l-6 6 6 6"></path></svg>
		</Button>
		<div class="font-display text-xl font-bold">Split entry</div>
	</div>

	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else if entry}
		<Card>
			<CardContent class="pt-4">
				<div class="text-xs font-semibold uppercase tracking-wide text-muted">Original</div>
				<div class="mt-1 flex items-baseline justify-between">
					<div class="font-semibold">{entry.description}</div>
					<div class="font-display font-bold">{formatMoney(Number(entry.amount), currency)}</div>
				</div>
				<div class="mt-0.5 text-xs text-muted">{entry.date}</div>
			</CardContent>
		</Card>

		<div class="flex flex-col gap-3">
			<div class="text-xs font-semibold uppercase tracking-wide text-muted">Parts</div>

			{#each parts as part, i (part.id)}
				<Card>
					<CardContent class="flex flex-col gap-3 pt-4">
						<div class="flex items-center justify-between">
							<div class="text-sm font-bold text-muted">Part {i + 1}</div>
							{#if parts.length > 2}
								<button
									type="button"
									class="text-xs font-semibold text-warn-foreground"
									onclick={() => removePart(part.id)}
								>Remove</button>
							{/if}
						</div>
						<div class="grid gap-2 @container @md:grid-cols-2">
							<div class="flex flex-col gap-1">
								<label for="part-amount-{part.id}" class="text-xs font-semibold text-muted">Amount ({currency})</label>
								<input
									id="part-amount-{part.id}"
									type="text"
									inputmode="decimal"
									placeholder="0.00"
									class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm font-display font-bold"
									bind:value={part.amount}
								/>
							</div>
							<div class="flex flex-col gap-1">
								<label for="part-category-{part.id}" class="text-xs font-semibold text-muted">Category</label>
								<CategoryInput
									id="part-category-{part.id}"
									categories={allCategories}
									class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm"
									bind:value={part.category}
								/>
							</div>
						</div>
						<div class="flex flex-col gap-1">
							<label for="part-note-{part.id}" class="text-xs font-semibold text-muted">Note</label>
							<input
								id="part-note-{part.id}"
								type="text"
								placeholder="Add a note"
								class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm"
								bind:value={part.note}
							/>
						</div>
					</CardContent>
				</Card>
			{/each}

			<button
				type="button"
				class="flex items-center justify-center gap-2 rounded-xl border border-dashed border-border py-3 text-sm font-bold text-primary"
				onclick={addPart}
			>
				<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
				Add part
			</button>
		</div>

		<div
			class="flex items-center justify-between rounded-xl px-4 py-3 {matches ? 'bg-primary-soft text-primary' : 'border border-warn-border bg-warn-bg text-warn-foreground'}"
		>
			<div class="text-sm font-semibold">Parts total</div>
			<div class="flex items-center gap-2">
				<span class="font-display font-bold">{formatMoney(partsTotal, currency)}</span>
				{#if matches}
					<span class="text-xs font-bold">&#10003; matches</span>
				{:else}
					<span class="text-xs font-semibold">{formatMoney(originalAmount - partsTotal, currency)} remaining</span>
				{/if}
			</div>
		</div>

		<Button disabled={!matches || busy} onclick={submit} class="w-full">
			{busy ? 'Saving...' : 'Save split'}
		</Button>
	{/if}
</div>
