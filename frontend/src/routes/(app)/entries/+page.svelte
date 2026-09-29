<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type AccountWithBalance, type Entry } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';

	let accounts = $state<AccountWithBalance[]>([]);
	let selectedAccountId = $state<string>('');
	let entries = $state<Entry[]>([]);
	let loading = $state(true);
	let entriesLoading = $state(false);
	let error = $state<string | null>(null);
	let voidingId = $state<string | null>(null);
	let voidReason = $state('');
	let actionError = $state<string | null>(null);

	const selectedAccount = $derived(accounts.find((a) => a.id === selectedAccountId));
	const sortedEntries = $derived([...entries].sort((a, b) => b.date.localeCompare(a.date)));

	// A voided entry and a reverted one both stop counting toward balances, so
	// both are dimmed; a pending one still counts but is not settled yet.
	function isDimmed(entry: Entry): boolean {
		return entry.voided_reason !== null || entry.bank_state === 'reverted';
	}

	onMount(async () => {
		try {
			accounts = await api.accounts.list();
			if (accounts.length > 0) {
				selectedAccountId = accounts[0].id;
				await loadEntries();
			}
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your accounts.';
		} finally {
			loading = false;
		}
	});

	async function loadEntries() {
		if (!selectedAccountId) return;
		entriesLoading = true;
		actionError = null;
		try {
			entries = await api.entries.list(selectedAccountId);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading entries.';
		} finally {
			entriesLoading = false;
		}
	}

	function replaceEntry(updated: Entry) {
		entries = entries.map((e) => (e.id === updated.id ? updated : e));
	}

	async function confirmEntry(id: string) {
		actionError = null;
		try {
			replaceEntry(await api.entries.confirm(id));
		} catch (err) {
			actionError = err instanceof ApiError ? err.message : 'Could not confirm this entry.';
		}
	}

	function startVoiding(id: string) {
		voidingId = id;
		voidReason = '';
		actionError = null;
	}

	async function submitVoid(id: string) {
		if (voidReason.trim().length === 0) {
			actionError = 'A reason is required to void an entry.';
			return;
		}
		actionError = null;
		try {
			replaceEntry(await api.entries.void(id, { reason: voidReason }));
			voidingId = null;
		} catch (err) {
			actionError = err instanceof ApiError ? err.message : 'Could not void this entry.';
		}
	}
</script>

<div class="flex flex-col gap-4 lg:max-w-2xl">
	<div class="font-display text-xl font-bold">Entries</div>

	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else if accounts.length === 0}
		<p class="text-sm text-muted">No accounts yet.</p>
	{:else}
		<select
			class="rounded-xl border border-border bg-card p-3 text-sm font-semibold"
			bind:value={selectedAccountId}
			onchange={loadEntries}
			aria-label="Account"
		>
			{#each accounts as account (account.id)}
				<option value={account.id}>{account.name} ({account.currency})</option>
			{/each}
		</select>

		{#if actionError}
			<Card class="border-warn-border bg-warn-bg">
				<CardContent class="pt-4 text-sm text-warn-foreground">{actionError}</CardContent>
			</Card>
		{/if}

		{#if entriesLoading}
			<p class="text-sm text-muted">Loading entries...</p>
		{:else if sortedEntries.length === 0}
			<p class="text-sm text-muted">No entries yet.</p>
		{:else}
			<div class="flex flex-col gap-2.5">
				{#each sortedEntries as entry (entry.id)}
					<Card class={isDimmed(entry) ? 'opacity-50' : ''}>
						<CardContent class="flex flex-col gap-2 pt-4">
							<div class="flex items-start justify-between gap-2">
								<div class="flex flex-col gap-1">
									<div class="text-sm font-semibold {isDimmed(entry) ? 'line-through' : ''}">
										{entry.description}
									</div>
									<div class="text-xs text-muted">{entry.date}</div>
									{#if entry.bank_state !== 'completed' || entry.category || entry.tags.length > 0}
										<div class="flex flex-wrap gap-1.5">
											{#if entry.bank_state === 'pending'}
												<Badge variant="warning">Pending</Badge>
											{:else if entry.bank_state === 'reverted'}
												<Badge>Reverted</Badge>
											{/if}
											{#if entry.category}
												<Badge>{entry.category}</Badge>
											{/if}
											{#each entry.tags as tag (tag)}
												<Badge>{tag}</Badge>
											{/each}
										</div>
									{/if}
									{#if entry.voided_reason !== null}
										<div class="text-xs text-warn-foreground">Voided: {entry.voided_reason}</div>
									{/if}
								</div>
								<div
									class="font-display text-sm font-bold {Number(entry.amount) >= 0 ? 'text-primary' : ''}"
								>
									{Number(entry.amount) >= 0 ? '+' : ''}{formatMoney(
										Number(entry.amount),
										entry.currency
									)}
								</div>
							</div>

							{#if entry.voided_reason === null}
								{#if voidingId === entry.id}
									<div class="flex flex-col gap-2 border-t border-border pt-2">
										<input
											type="text"
											placeholder="Reason for voiding"
											class="rounded-lg border border-border bg-page p-2 text-sm"
											bind:value={voidReason}
										/>
										<div class="flex gap-2">
											<Button size="sm" onclick={() => submitVoid(entry.id)}>Confirm void</Button>
											<Button size="sm" variant="ghost" onclick={() => (voidingId = null)}>Cancel</Button>
										</div>
									</div>
								{:else}
									<div class="flex gap-2 border-t border-border pt-2">
										{#if entry.source === 'manual' && !entry.confirmed}
											<Button size="sm" variant="outline" onclick={() => confirmEntry(entry.id)}>Confirm</Button>
										{/if}
										<Button size="sm" variant="ghost" onclick={() => startVoiding(entry.id)}>Void</Button>
									</div>
								{/if}
							{/if}
						</CardContent>
					</Card>
				{/each}
			</div>
		{/if}
	{/if}
</div>
