<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type Account, type AccountWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import NewAccountForm from '$lib/components/accounts/new-account-form.svelte';
	import AccountCard from '$lib/components/accounts/account-card.svelte';

	let accounts = $state<AccountWithBalance[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let creating = $state(false);

	const KIND_ORDER: Record<string, number> = { own: 0, investment: 1, person: 2, outside: 3 };
	const KIND_LABEL: Record<string, string> = { own: 'Cash accounts', investment: 'Investments', person: 'People', outside: 'External' };

	const groupedAccounts = $derived.by(() => {
		const byCurrency = new Map<string, Map<string, AccountWithBalance[]>>();
		for (const account of accounts) {
			if (!byCurrency.has(account.currency)) byCurrency.set(account.currency, new Map());
			const byKind = byCurrency.get(account.currency)!;
			if (!byKind.has(account.kind)) byKind.set(account.kind, []);
			byKind.get(account.kind)!.push(account);
		}
		return [...byCurrency.entries()].map(([currency, byKind]) => ({
			currency,
			kinds: [...byKind.entries()]
				.sort(([a], [b]) => (KIND_ORDER[a] ?? 9) - (KIND_ORDER[b] ?? 9))
				.map(([kind, list]) => ({ kind, label: KIND_LABEL[kind] ?? kind, list }))
		}));
	});

	onMount(load);

	async function load() {
		loading = true;
		error = null;
		try {
			accounts = await api.accounts.list();
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your accounts.';
		} finally {
			loading = false;
		}
	}

	function addAccount(account: Account) {
		creating = false;
		accounts = [...accounts, { ...account, balance: account.opening_balance }];
	}

	function replaceAccount(updated: AccountWithBalance) {
		accounts = accounts.map((account) => (account.id === updated.id ? updated : account));
	}
</script>

<div class="flex flex-col gap-4">
	<div class="flex items-center justify-between gap-3">
		<div class="font-display text-xl font-bold">Accounts</div>
		{#if !loading && !error}
			<Button size="sm" class="max-lg:h-11" onclick={() => (creating = !creating)}>
				{creating ? 'Cancel' : 'New account'}
			</Button>
		{/if}
	</div>

	{#if loading}
		<p class="text-sm text-muted">Loading...</p>
	{:else if error}
		<Card class="border-warn-border bg-warn-bg">
			<CardContent class="pt-5 text-sm text-warn-foreground">{error}</CardContent>
		</Card>
	{:else}
		{#if creating}
			<NewAccountForm oncreated={addAccount} oncancel={() => (creating = false)} />
		{/if}

		{#if accounts.length === 0}
			<p class="text-sm text-muted">No accounts yet. Create one to get started.</p>
		{:else}
			<div class="flex flex-col gap-6">
				{#each groupedAccounts as group (group.currency)}
					<div class="flex flex-col gap-3">
						{#if groupedAccounts.length > 1}
							<div class="text-xs font-semibold tracking-wide text-muted uppercase">{group.currency}</div>
						{/if}
						{#each group.kinds as section (section.kind)}
							<div class="flex flex-col gap-2">
								{#if group.kinds.length > 1}
									<div class="text-xs text-muted-2 font-semibold">{section.label}</div>
								{/if}
								<div class="flex flex-col gap-2.5 lg:grid lg:grid-cols-2 lg:gap-4 xl:grid-cols-3">
									{#each section.list as account (account.id)}
										<AccountCard {account} onchanged={replaceAccount} />
									{/each}
								</div>
							</div>
						{/each}
					</div>
				{/each}
			</div>
		{/if}
	{/if}
</div>
