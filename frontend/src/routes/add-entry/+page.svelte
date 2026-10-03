<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, ApiError, type AccountWithBalance, type EntryKind, type PotWithBalance } from '$lib/api';
	import { Button } from '$lib/components/ui/button';
	import NavSidebar from '$lib/components/nav-sidebar.svelte';

	let accounts = $state<AccountWithBalance[]>([]);
	let pots = $state<PotWithBalance[]>([]);
	let loading = $state(true);
	let loadError = $state<string | null>(null);
	let submitting = $state(false);
	let submitError = $state<string | null>(null);

	let isTransfer = $state(false);
	let kind = $state<EntryKind>('expense');
	let accountId = $state('');
	let fromAccountId = $state('');
	let toAccountId = $state('');
	let amount = $state('');
	let amountReceived = $state('');
	let description = $state('');
	let date = $state(new Date().toISOString().slice(0, 10));
	let category = $state('');
	let tagsInput = $state('');
	let potId = $state('');
	let showMore = $state(false);

	// An archived account is done being used day to day: it stays visible
	// everywhere its own history already exists (Entries, Stats), but
	// never as a destination for something new.
	const activeAccounts = $derived(accounts.filter((a) => !a.archived));

	const fromAccount = $derived(accounts.find((a) => a.id === fromAccountId));
	const toAccount = $derived(accounts.find((a) => a.id === toAccountId));
	const isCrossCurrency = $derived(
		isTransfer && !!fromAccount && !!toAccount && fromAccount.currency !== toAccount.currency
	);
	const selectedAccount = $derived(accounts.find((a) => a.id === accountId));
	const potsForAccount = $derived(pots.filter((p) => p.currency === selectedAccount?.currency));
	const toAccountOptions = $derived(activeAccounts.filter((a) => a.id !== fromAccountId));

	onMount(async () => {
		try {
			[accounts, pots] = await Promise.all([api.accounts.list(), api.pots.list()]);
			if (activeAccounts.length > 0) {
				accountId = activeAccounts[0].id;
				fromAccountId = activeAccounts[0].id;
				toAccountId = activeAccounts.find((a) => a.id !== activeAccounts[0].id)?.id ?? '';
			}
		} catch (err) {
			loadError = err instanceof ApiError ? err.message : 'Something went wrong loading your accounts.';
		} finally {
			loading = false;
		}
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		submitError = null;

		if (Number(amount) < 0) {
			submitError = 'Amount must be zero or positive.';
			return;
		}

		submitting = true;
		try {
			if (isTransfer) {
				if (!fromAccountId || !toAccountId) {
					submitError = 'Pick both accounts.';
					return;
				}
				await api.transfers.create({
					from_account_id: fromAccountId,
					to_account_id: toAccountId,
					date,
					amount_sent: amount,
					amount_received: isCrossCurrency ? amountReceived : amount,
					description
				});
			} else {
				if (!accountId) {
					submitError = 'Pick an account.';
					return;
				}
				const entry = await api.entries.record({ account_id: accountId, date, kind, amount, description });
				const tags = tagsInput
					.split(',')
					.map((t) => t.trim())
					.filter((t) => t.length > 0);
				if (category.trim() || tags.length > 0 || potId) {
					// The entry above is already saved at this point (its own
					// account balance is affected either way), so a failure here
					// -- most commonly the pot check refusing to go negative --
					// must not leave that entry behind untagged with no sign
					// anything was saved. Void it and surface the real error.
					try {
						await api.entries.updateMetadata(entry.id, {
							category: category.trim() || null,
							tags,
							note: null,
							pot_id: potId || null
						});
					} catch (metadataErr) {
						await api.entries.void(entry.id, { reason: 'Entry could not be tagged as requested' }).catch(() => {});
						throw metadataErr;
					}
				}
			}
			goto('/');
		} catch (err) {
			submitError = err instanceof ApiError ? err.message : 'Could not save this entry.';
		} finally {
			submitting = false;
		}
	}
</script>

<div class="flex h-dvh bg-page">
	<NavSidebar />
	<div class="relative flex flex-1 items-center justify-center lg:bg-foreground/25">
		<div
			class="flex h-dvh w-full flex-col bg-page lg:h-auto lg:max-h-[85vh] lg:w-[640px] lg:rounded-2xl lg:bg-card lg:shadow-2xl"
		>
			<div
				class="flex flex-none items-center justify-between gap-3 px-5 pt-[18px] pb-2 lg:border-b lg:border-border lg:px-7 lg:py-5"
			>
				<div class="font-display text-[17px] font-bold lg:text-lg">Add entry</div>
				<button
					type="button"
					onclick={() => history.back()}
					aria-label="Close"
					class="flex h-8 w-8 items-center justify-center rounded-full hover:bg-row-hover"
				>
					<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
						><path d="M6 6l12 12M18 6L6 18"></path></svg
					>
				</button>
			</div>

			<div class="flex-1 overflow-y-auto px-5 pb-6 lg:px-7 lg:pt-6 lg:pb-6">
				{#if loading}
					<p class="text-sm text-muted">Loading...</p>
				{:else if loadError}
					<p class="text-sm text-warn-foreground">{loadError}</p>
				{:else if activeAccounts.length === 0}
					<p class="text-sm text-muted">
						{accounts.length === 0 ? 'Open an account first.' : 'Unarchive an account first.'}
					</p>
				{:else}
					<form id="add-entry-form" class="flex flex-col gap-4" onsubmit={submit}>
				{#if !isTransfer}
					<div class="flex rounded-xl bg-app p-1">
						<button
							type="button"
							class="flex-1 rounded-lg py-2.5 text-sm font-bold {kind === 'expense' ? 'bg-card' : 'text-muted-2'}"
							onclick={() => (kind = 'expense')}
						>
							Expense
						</button>
						<button
							type="button"
							class="flex-1 rounded-lg py-2.5 text-sm font-bold {kind === 'income' ? 'bg-card' : 'text-muted-2'}"
							onclick={() => (kind = 'income')}
						>
							Money in
						</button>
					</div>
				{/if}

				<label class="flex flex-col gap-2">
					<span class="text-xs font-semibold tracking-wide text-muted uppercase">Amount</span>
					<input
						type="text"
						inputmode="decimal"
						placeholder="0.00"
						class="rounded-xl border border-border bg-card p-4 font-display text-2xl font-bold"
						bind:value={amount}
						required
					/>
				</label>

				{#if !isTransfer}
					<label class="flex flex-col gap-2">
						<span class="text-xs font-semibold tracking-wide text-muted uppercase">Account</span>
						<select class="rounded-xl border border-border bg-card p-3 text-sm" bind:value={accountId}>
							{#each activeAccounts as account (account.id)}
								<option value={account.id}>{account.name} ({account.currency})</option>
							{/each}
						</select>
					</label>

					<label class="flex flex-col gap-2">
						<span class="text-xs font-semibold tracking-wide text-muted uppercase">Description</span>
						<input
							type="text"
							placeholder="What was it?"
							class="rounded-xl border border-border bg-card p-3 text-sm"
							bind:value={description}
							required
						/>
					</label>
				{/if}

				<label
					class="flex cursor-pointer items-center gap-3 rounded-xl border border-border bg-card p-4"
				>
					<input type="checkbox" bind:checked={isTransfer} class="h-4 w-4" />
					<div>
						<div class="text-sm font-semibold">This is a transfer</div>
						<div class="text-xs text-muted-2">Moving value to another account, not spending it</div>
					</div>
				</label>

				{#if isTransfer}
					<label class="flex flex-col gap-2">
						<span class="text-xs font-semibold tracking-wide text-muted uppercase">From account</span>
						<select class="rounded-xl border border-border bg-card p-3 text-sm" bind:value={fromAccountId}>
							{#each activeAccounts as account (account.id)}
								<option value={account.id}>{account.name} ({account.currency})</option>
							{/each}
						</select>
					</label>

					<label class="flex flex-col gap-2">
						<span class="text-xs font-semibold tracking-wide text-muted uppercase">To account</span>
						<select class="rounded-xl border border-border bg-card p-3 text-sm" bind:value={toAccountId}>
							{#each toAccountOptions as account (account.id)}
								<option value={account.id}>{account.name} ({account.currency})</option>
							{/each}
						</select>
					</label>

					{#if isCrossCurrency}
						<label class="flex flex-col gap-2">
							<span class="text-xs font-semibold tracking-wide text-muted uppercase">
								Amount received ({toAccount?.currency})
							</span>
							<input
								type="text"
								inputmode="decimal"
								placeholder="0.00"
								class="rounded-xl border border-border bg-card p-3 text-sm"
								bind:value={amountReceived}
								required
							/>
						</label>
					{/if}

					<label class="flex flex-col gap-2">
						<span class="text-xs font-semibold tracking-wide text-muted uppercase">Description</span>
						<input
							type="text"
							placeholder="What was it?"
							class="rounded-xl border border-border bg-card p-3 text-sm"
							bind:value={description}
							required
						/>
					</label>
				{/if}

				{#if !isTransfer}
					<button
						type="button"
						class="flex items-center gap-1.5 self-start text-sm font-bold text-primary"
						onclick={() => (showMore = !showMore)}
					>
						{showMore ? 'Fewer options' : 'More options (category, tags, pot)'}
					</button>

					{#if showMore}
						<div class="flex flex-col gap-4 rounded-xl border border-border bg-card p-4">
							<label class="flex flex-col gap-2">
								<span class="text-xs font-semibold text-muted">Category</span>
								<input
									type="text"
									placeholder="Groceries"
									class="rounded-lg border border-border bg-page p-2.5 text-sm"
									bind:value={category}
								/>
							</label>
							<label class="flex flex-col gap-2">
								<span class="text-xs font-semibold text-muted">Tags (comma separated)</span>
								<input
									type="text"
									placeholder="trip, camera"
									class="rounded-lg border border-border bg-page p-2.5 text-sm"
									bind:value={tagsInput}
								/>
							</label>
							{#if potsForAccount.length > 0}
								<label class="flex flex-col gap-2">
									<span class="text-xs font-semibold text-muted">Pot</span>
									<select class="rounded-lg border border-border bg-page p-2.5 text-sm" bind:value={potId}>
										<option value="">None</option>
										{#each potsForAccount as pot (pot.id)}
											<option value={pot.id}>{pot.name}</option>
										{/each}
									</select>
								</label>
							{/if}
						</div>
					{/if}
				{/if}

				<label class="flex flex-col gap-2">
					<span class="text-xs font-semibold tracking-wide text-muted uppercase">Date</span>
					<input type="date" class="rounded-xl border border-border bg-card p-3 text-sm" bind:value={date} required />
				</label>

				{#if submitError}
					<p class="text-sm text-warn-foreground">{submitError}</p>
				{/if}

				<Button type="submit" disabled={submitting} class="w-full lg:hidden">
					{submitting ? 'Saving...' : 'Save entry'}
				</Button>
			</form>
		{/if}
			</div>

			{#if !loading && !loadError && accounts.length > 0}
				<div class="hidden flex-none items-center justify-end gap-2.5 border-t border-border px-7 py-5 lg:flex">
					<Button type="button" variant="outline" onclick={() => history.back()}>Cancel</Button>
					<Button type="submit" form="add-entry-form" disabled={submitting}>
						{submitting ? 'Saving...' : 'Save entry'}
					</Button>
				</div>
			{/if}
		</div>
	</div>
</div>
