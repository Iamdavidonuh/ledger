<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		ApiError,
		type AcceptAsTransferRequest,
		type AccountWithBalance,
		type Entry,
		type Import,
		type ImportQueueRow,
		type NormalQueueRow
	} from '$lib/api';
	import { Button } from '$lib/components/ui/button';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { formatMoney } from '$lib/format';
	import { bulkGate, gateMessage, groupQueue } from '$lib/import/queue';
	import CleanRow from './clean-row.svelte';
	import DuplicateRow from './duplicate-row.svelte';
	import ReversalRow from './reversal-row.svelte';
	import SectionHeading from './section-heading.svelte';

	// The review queue for one import. Every row waits here until the person
	// accepts, moves or discards it; the server applies each action in one
	// transaction, so after every action the queue and the account's entries
	// are read again rather than patched locally (accepting a row also
	// rewrites the matches of its siblings). One action runs at a time, and a
	// category being saved always finishes before the next action starts, so
	// two requests never race on the same row.
	let {
		importId,
		account,
		accounts,
		intro,
		onexit
	}: {
		importId: string;
		account: AccountWithBalance;
		accounts: AccountWithBalance[];
		intro?: string;
		onexit: () => void;
	} = $props();

	const uid = $props.id();
	const gateId = `gate-${uid}`;

	let statement = $state<Import | null>(null);
	let rows = $state<ImportQueueRow[]>([]);
	let entries = $state<Entry[]>([]);
	let loading = $state(true);
	let loadError = $state<string | null>(null);
	let actionError = $state<string | null>(null);
	let busyRowId = $state<string | null>(null);
	let bulkBusy = $state(false);
	let confirmingDiscardAll = $state(false);
	let doneMessage = $state<string | null>(null);
	let alertElement = $state<HTMLElement | null>(null);
	let pendingSave: Promise<void> = Promise.resolve();

	const locked = $derived(busyRowId !== null || bulkBusy);
	const groups = $derived(groupQueue(rows));
	const gate = $derived(bulkGate(groups));
	const gateText = $derived(gateMessage(gate));
	const entriesById = $derived(new Map(entries.map((entry) => [entry.id, entry])));
	const rowsById = $derived(new Map(rows.map((row) => [row.id, row])));
	const categories = $derived(
		[...new Set(entries.flatMap((entry) => (entry.category ? [entry.category] : [])))].sort((a, b) =>
			a.localeCompare(b)
		)
	);
	const finished = $derived(!loading && loadError === null && rows.length === 0);
	// A statement that came in with nothing to import (rows_read from the
	// import record itself, not the current queue) is a different situation
	// from one that has just been fully reviewed, and reads differently.
	const emptyStatement = $derived(finished && statement?.rows_read === 0);

	// An error above a long list is invisible on a phone, so bring it into view.
	$effect(() => {
		if (actionError && alertElement) alertElement.scrollIntoView({ block: 'nearest' });
	});

	function matchedEntries(row: NormalQueueRow): Entry[] {
		return row.matched_entry_ids.flatMap((id) => {
			const entry = entriesById.get(id);
			return entry ? [entry] : [];
		});
	}

	function matchedRows(row: NormalQueueRow): NormalQueueRow[] {
		return row.matched_queue_row_ids.flatMap((id) => {
			const sibling = rowsById.get(id);
			return sibling && sibling.kind === 'Normal' ? [sibling] : [];
		});
	}

	function messageFor(err: unknown, fallback: string): string {
		return err instanceof ApiError ? err.message : fallback;
	}

	async function refresh() {
		const [nextRows, nextEntries] = await Promise.all([
			api.imports.queue(importId),
			api.entries.list(account.id)
		]);
		rows = nextRows;
		entries = nextEntries;
	}

	// Called after a change the server already accepted. A failed re-read must
	// not be reported as a failed change.
	async function reloadAfterChange() {
		try {
			await refresh();
		} catch {
			actionError = 'The change was saved, but this list could not be refreshed. Reload the page.';
		}
	}

	onMount(async () => {
		try {
			const [loaded] = await Promise.all([api.imports.get(importId), refresh()]);
			statement = loaded;
			if (loaded.rows_read === 0) {
				// Nothing was ever staged to review, so no row is ever left to
				// resolve, and nothing else would ever mark this import
				// complete. Left alone, it would stay open forever and block
				// this account from taking another file. Discarding an empty
				// queue is a no-op either way, so this is safe to do quietly.
				try {
					await api.imports.discardAll(importId);
				} catch {
					// Best effort: if this fails, the account stays blocked
					// until this review is opened again.
				}
			}
		} catch (err) {
			loadError = messageFor(err, 'Something went wrong loading this review.');
		} finally {
			loading = false;
		}
	});

	async function run(rowId: string, action: () => Promise<unknown>, failure: string) {
		if (locked) return;
		busyRowId = rowId;
		actionError = null;
		try {
			await pendingSave;
			await action();
			await reloadAfterChange();
		} catch (err) {
			actionError = messageFor(err, failure);
		} finally {
			busyRowId = null;
		}
	}

	const accept = (row: NormalQueueRow, category: string | undefined) =>
		run(
			row.id,
			() => api.imports.accept(importId, row.id, category ? { category } : undefined),
			'Could not accept this row.'
		);

	const moveToAccount = (row: NormalQueueRow, request: AcceptAsTransferRequest) =>
		run(
			row.id,
			() => api.imports.acceptAsTransfer(importId, row.id, request),
			'Could not save this transfer.'
		);

	const discard = (row: ImportQueueRow) =>
		run(row.id, () => api.imports.discard(importId, row.id), 'Could not discard this row.');

	const resolveReversal = (row: ImportQueueRow) =>
		run(row.id, () => api.imports.resolveRevert(importId, row.id), 'Could not mark this entry as reverted.');

	// Saves happen one after another, and every other action waits for them,
	// so leaving the field and pressing Accept all straight away is safe.
	function saveCategory(row: NormalQueueRow, category: string | null) {
		pendingSave = pendingSave.then(async () => {
			try {
				const updated = await api.imports.updateCategory(importId, row.id, { category });
				rows = rows.map((candidate) =>
					candidate.kind === 'Normal' && candidate.id === row.id
						? { ...candidate, category: updated.category }
						: candidate
				);
			} catch (err) {
				actionError = messageFor(err, 'Could not save this category.');
			}
		});
	}

	async function acceptAll() {
		if (locked) return;
		bulkBusy = true;
		actionError = null;
		try {
			await pendingSave;
			const { accepted } = await api.imports.acceptAll(importId);
			doneMessage = `${accepted} ${accepted === 1 ? 'entry was' : 'entries were'} added to your ledger.`;
			await reloadAfterChange();
		} catch (err) {
			actionError = messageFor(err, 'Could not accept these rows.');
		} finally {
			bulkBusy = false;
		}
	}

	async function discardAll() {
		if (locked) return;
		bulkBusy = true;
		actionError = null;
		try {
			await pendingSave;
			const { discarded } = await api.imports.discardAll(importId);
			doneMessage = `${discarded} ${discarded === 1 ? 'row was' : 'rows were'} discarded. Nothing was added to your ledger.`;
			confirmingDiscardAll = false;
			await reloadAfterChange();
		} catch (err) {
			actionError = messageFor(err, 'Could not discard these rows.');
		} finally {
			bulkBusy = false;
		}
	}
</script>

<div class="flex flex-col gap-5">
	<div class="flex items-center gap-2">
		<Button variant="ghost" size="icon" class="max-lg:h-11 max-lg:w-11" aria-label="Back to imports" onclick={onexit}>
			<svg
				width="18"
				height="18"
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2.2"
				stroke-linecap="round"
				stroke-linejoin="round"
				aria-hidden="true"><path d="M15 6l-6 6 6 6"></path></svg
			>
		</Button>
		<div class="min-w-0">
			<h1 class="truncate font-display text-xl font-bold">Review import</h1>
			<p class="truncate text-xs text-muted-2">
				{account.name}{statement ? `, ${statement.file_name}` : ''}
			</p>
		</div>
	</div>

	{#if actionError}
		<p
			bind:this={alertElement}
			class="rounded-xl border border-warn-border bg-warn-bg p-3 text-sm text-warn-foreground"
			role="alert"
		>
			{actionError}
		</p>
	{/if}

	{#if loading}
		<p class="text-sm text-muted" role="status">Loading this review...</p>
	{:else if loadError}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground" role="alert">{loadError}</CardContent>
		</Card>
	{:else if emptyStatement}
		<Card class="flex flex-col items-center gap-3 p-8 text-center">
			<span class="flex h-12 w-12 items-center justify-center rounded-full bg-page text-muted">
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"><path d="M14 3v5a2 2 0 002 2h5"></path><path
						d="M17 21H7a2 2 0 01-2-2V5a2 2 0 012-2h7l5 5v11a2 2 0 01-2 2z"
					></path></svg
				>
			</span>
			<h2 class="font-display text-lg font-bold">Nothing to import</h2>
			<p class="max-w-sm text-sm text-muted">
				This statement passed its balance check but had no rows to bring in. Nothing was added to your ledger.
			</p>
			<div class="flex flex-wrap justify-center gap-3">
				<Button variant="outline" class="max-lg:h-11" onclick={onexit}>Back to imports</Button>
			</div>
		</Card>
	{:else if finished}
		<Card class="flex flex-col items-center gap-3 p-8 text-center">
			<span class="flex h-12 w-12 items-center justify-center rounded-full bg-primary-soft text-primary">
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.4"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"></path></svg
				>
			</span>
			<h2 class="font-display text-lg font-bold">Nothing left to review</h2>
			<p class="max-w-sm text-sm text-muted">
				{doneMessage ?? 'This import is finished.'} You can fix anything later from the Entries screen.
			</p>
			<div class="flex flex-wrap justify-center gap-3">
				<Button href="/entries" class="max-lg:h-11">See entries</Button>
				<Button variant="outline" class="max-lg:h-11" onclick={onexit}>Back to imports</Button>
			</div>
		</Card>
	{:else}
		{#if intro}
			<p class="text-sm text-muted">{intro}</p>
		{/if}
		<p class="text-sm font-semibold lg:hidden">
			{rows.length}
			{rows.length === 1 ? 'row' : 'rows'} to review.
			<span class="font-normal text-muted">{gateText ?? 'Everything left is ready to accept.'}</span>
		</p>

		<div class="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem] lg:items-start">
			<aside class="order-last lg:sticky lg:top-0 lg:order-none lg:col-start-2 lg:row-start-1">
				<Card class="flex flex-col gap-4 p-4">
					<div class="flex flex-col gap-1">
						<p class="font-display text-base font-bold">
							{rows.length}
							{rows.length === 1 ? 'row' : 'rows'} to review
						</p>
						{#if statement}
							<p class="text-xs text-muted-2">
								The statement balance check passed. It closes at {formatMoney(
									Number(statement.closing_balance),
									statement.currency
								)}.
							</p>
						{/if}
					</div>

					<dl class="grid grid-cols-3 gap-2 text-center">
						<div class="flex flex-col-reverse rounded-xl bg-page p-2">
							<dt class="text-[11px] text-muted-2">Ready</dt>
							<dd class="font-display text-lg font-bold text-primary">{groups.clean.length}</dd>
						</div>
						<div class="flex flex-col-reverse rounded-xl bg-page p-2">
							<dt class="text-[11px] text-muted-2">Duplicates</dt>
							<dd class="font-display text-lg font-bold">{groups.needsDecision.length}</dd>
						</div>
						<div class="flex flex-col-reverse rounded-xl bg-page p-2">
							<dt class="text-[11px] text-muted-2">Reversals</dt>
							<dd class="font-display text-lg font-bold">{groups.candidates.length}</dd>
						</div>
					</dl>

					<div class="flex flex-col gap-3">
						<Button
							disabled={gate.blocked || groups.clean.length === 0 || locked}
							onclick={acceptAll}
							class="w-full max-lg:h-12"
							aria-describedby={gateText ? gateId : undefined}
						>
							Accept {groups.clean.length} ready {groups.clean.length === 1 ? 'row' : 'rows'}
						</Button>
						{#if gateText}
							<p id={gateId} class="text-xs text-muted-2">{gateText}</p>
						{/if}

						{#if confirmingDiscardAll}
							<div class="flex flex-col gap-3 rounded-xl border border-warn-border bg-warn-bg p-3">
								<p class="text-sm text-warn-foreground">
									Discard all {rows.length}
									{rows.length === 1 ? 'row' : 'rows'}? Nothing is added to your ledger.
								</p>
								<div class="flex flex-wrap gap-3">
									<Button
										size="sm"
										variant="outline"
										class="border-warn-icon text-warn-foreground max-lg:h-11"
										disabled={locked}
										onclick={discardAll}>Yes, discard all</Button
									>
									<Button
										size="sm"
										variant="ghost"
										class="max-lg:h-11"
										disabled={locked}
										onclick={() => (confirmingDiscardAll = false)}>Keep reviewing</Button
									>
								</div>
							</div>
						{:else}
							<Button
								variant="outline"
								class="w-full max-lg:h-12"
								disabled={locked}
								onclick={() => (confirmingDiscardAll = true)}>Discard everything</Button
							>
						{/if}
					</div>
				</Card>
			</aside>

			<div class="flex min-w-0 flex-col gap-6 lg:col-start-1 lg:row-start-1">
				{#if groups.needsDecision.length > 0}
					<section class="flex flex-col gap-2.5">
						<SectionHeading
							title="Possible duplicates"
							count={groups.needsDecision.length}
							hint="Decide each one before accepting the rest"
						/>
						{#each groups.needsDecision as row (row.id)}
							<DuplicateRow
								{row}
								{account}
								{accounts}
								{categories}
								matchedEntries={matchedEntries(row)}
								matchedRows={matchedRows(row)}
								busy={locked}
								onaccept={(category) => accept(row, category)}
								ondiscard={() => discard(row)}
								ontransfer={(request) => moveToAccount(row, request)}
							/>
						{/each}
					</section>
				{/if}

				{#if groups.candidates.length > 0}
					<section class="flex flex-col gap-2.5">
						<SectionHeading
							title="Bank reversals"
							count={groups.candidates.length}
							hint="Never applied automatically"
						/>
						{#each groups.candidates as row (row.id)}
							<ReversalRow
								{row}
								suggestedEntry={row.suggested_entry_id
									? entriesById.get(row.suggested_entry_id)
									: undefined}
								busy={locked}
								onresolve={() => resolveReversal(row)}
								ondiscard={() => discard(row)}
							/>
						{/each}
					</section>
				{/if}

				{#if groups.clean.length > 0}
					<section class="@container flex flex-col gap-2.5">
						<SectionHeading title="Ready to accept" count={groups.clean.length} />
						<div class="grid gap-2.5 @2xl:grid-cols-2">
							{#each groups.clean as row (row.id)}
								<CleanRow
									{row}
									{account}
									{accounts}
									{categories}
									busy={locked}
									onaccept={(category) => accept(row, category)}
									ondiscard={() => discard(row)}
									onsetcategory={(category) => saveCategory(row, category)}
									ontransfer={(request) => moveToAccount(row, request)}
								/>
							{/each}
						</div>
					</section>
				{/if}
			</div>
		</div>
	{/if}
</div>
