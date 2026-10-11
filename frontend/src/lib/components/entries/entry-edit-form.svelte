<script lang="ts">
	import { untrack } from 'svelte';
	import { api, ApiError, type AccountWithBalance, type Entry, type EntryKind, type PotWithBalance } from '$lib/api';
	import { Button } from '$lib/components/ui/button';

	// Category, tags, note and pot are always editable, even on an imported
	// or confirmed entry -- only amount is locked once an entry is anything
	// but an unconfirmed manual one (see Ledger::edit_manual_entry_amount),
	// so this form's amount section only appears when that's actually true.
	let {
		entry,
		pots,
		accounts = [],
		onupdated,
		oncancel
	}: {
		entry: Entry;
		pots: PotWithBalance[];
		accounts?: AccountWithBalance[];
		onupdated: (entry: Entry) => void;
		oncancel: () => void;
	} = $props();

	const uid = $props.id();
	const amountEditable = $derived(entry.source === 'manual' && !entry.confirmed);
	const potsForEntry = $derived(pots.filter((p) => p.currency === entry.currency));

	let kind = $state<EntryKind>(untrack(() => (Number(entry.amount) >= 0 ? 'income' : 'expense')));
	let amount = $state(untrack(() => Math.abs(Number(entry.amount)).toString()));
	let category = $state(untrack(() => entry.category ?? ''));
	let tagsInput = $state(untrack(() => entry.tags.join(', ')));
	let potId = $state(untrack(() => entry.pot_id ?? ''));
	let note = $state(untrack(() => entry.note ?? ''));
	let isTransfer = $state(untrack(() => entry.transfer_account_id !== null));
	let transferAccountId = $state(untrack(() => entry.transfer_account_id ?? ''));
	let busy = $state(false);
	let error = $state<string | null>(null);

	const otherAccounts = $derived(accounts.filter((a) => a.id !== entry.account_id && a.currency === entry.currency));

	const amountText = $derived(amount.trim().replace(',', '.'));
	const amountValid = $derived(!amountEditable || (amountText !== '' && Number(amountText) >= 0));

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!amountValid) return;
		busy = true;
		error = null;
		try {
			let updated = entry;
			if (amountEditable && amountText !== Math.abs(Number(entry.amount)).toString()) {
				updated = await api.entries.editAmount(entry.id, { kind, amount: amountText });
			}
			const tags = tagsInput
				.split(',')
				.map((t) => t.trim())
				.filter((t) => t.length > 0);
			updated = await api.entries.updateMetadata(entry.id, {
				category: category.trim() || null,
				tags,
				note: note.trim() || null,
				pot_id: potId || null,
				...(isTransfer && transferAccountId
					? { transfer_account_id: transferAccountId }
					: !isTransfer && entry.transfer_account_id !== null
						? { clear_transfer: true }
						: {})
			});
			onupdated(updated);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not save these changes.';
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-3 border-t border-border pt-3" onsubmit={submit}>
	{#if amountEditable}
		<div class="flex gap-2">
			<div class="flex rounded-lg bg-page p-0.5">
				<button
					type="button"
					class="rounded-md px-3 py-1.5 text-xs font-bold {kind === 'expense' ? 'bg-card' : 'text-muted-2'}"
					onclick={() => (kind = 'expense')}
				>
					Expense
				</button>
				<button
					type="button"
					class="rounded-md px-3 py-1.5 text-xs font-bold {kind === 'income' ? 'bg-card' : 'text-muted-2'}"
					onclick={() => (kind = 'income')}
				>
					Money in
				</button>
			</div>
			<input
				type="text"
				inputmode="decimal"
				class="min-h-9 flex-1 rounded-lg border border-border bg-page px-2.5 text-sm"
				aria-invalid={!amountValid}
				bind:value={amount}
			/>
		</div>
	{/if}

	{#if accounts.length > 0}
		<label for="transfer-toggle-{uid}" class="flex cursor-pointer items-center gap-3 rounded-lg border border-border bg-page px-3 py-2.5">
			<input id="transfer-toggle-{uid}" type="checkbox" class="sr-only" bind:checked={isTransfer} />
			<span
				class="relative inline-flex h-5 w-9 flex-shrink-0 rounded-full transition-colors {isTransfer ? 'bg-primary' : 'bg-border'}"
				aria-hidden="true"
			>
				<span
					class="inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform {isTransfer ? 'translate-x-4' : 'translate-x-0.5'} mt-0.5"
				></span>
			</span>
			<div>
				<div class="text-sm font-semibold">This is a transfer</div>
				<div class="text-xs text-muted">Moving value to another account, not spending</div>
			</div>
		</label>
		{#if isTransfer && otherAccounts.length > 0}
			<div class="flex flex-col gap-1">
				<label for="transfer-account-{uid}" class="text-xs font-semibold text-muted">Other account</label>
				<select id="transfer-account-{uid}" class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm" bind:value={transferAccountId}>
					<option value="">— pick account —</option>
					{#each otherAccounts as a (a.id)}
						<option value={a.id}>{a.name}</option>
					{/each}
				</select>
			</div>
		{:else if isTransfer}
			<p class="text-xs text-muted">No other {entry.currency} accounts to link.</p>
		{/if}
	{/if}

	<div class="grid gap-2 @md:grid-cols-2">
		<div class="flex flex-col gap-1">
			<label for="category-{uid}" class="text-xs font-semibold text-muted">Category</label>
			<input
				id="category-{uid}"
				type="text"
				placeholder="Groceries"
				class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm"
				bind:value={category}
			/>
		</div>
		<div class="flex flex-col gap-1">
			<label for="tags-{uid}" class="text-xs font-semibold text-muted">Tags</label>
			<input
				id="tags-{uid}"
				type="text"
				placeholder="trip, camera"
				class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm"
				bind:value={tagsInput}
			/>
		</div>
		{#if potsForEntry.length > 0}
			<div class="flex flex-col gap-1">
				<label for="pot-{uid}" class="text-xs font-semibold text-muted">Pot</label>
				<select
					id="pot-{uid}"
					class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm"
					bind:value={potId}
				>
					<option value="">None</option>
					{#each potsForEntry as pot (pot.id)}
						<option value={pot.id}>{pot.name}</option>
					{/each}
				</select>
			</div>
		{/if}
		<div class="flex flex-col gap-1">
			<label for="note-{uid}" class="text-xs font-semibold text-muted">Note</label>
			<input
				id="note-{uid}"
				type="text"
				placeholder="Context for later"
				class="min-h-9 rounded-lg border border-border bg-page px-2.5 text-sm"
				bind:value={note}
			/>
		</div>
	</div>

	{#if error}
		<p class="text-xs text-warn-foreground">{error}</p>
	{/if}

	<div class="flex gap-2">
		<Button type="submit" size="sm" disabled={busy || !amountValid}>{busy ? 'Saving...' : 'Save'}</Button>
		<Button type="button" size="sm" variant="ghost" disabled={busy} onclick={oncancel}>Cancel</Button>
	</div>
</form>
