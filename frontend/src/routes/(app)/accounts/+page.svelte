<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError, type Account, type AccountWithBalance } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';
	import NewAccountForm from '$lib/components/accounts/new-account-form.svelte';

	let accounts = $state<AccountWithBalance[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let creating = $state(false);

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
			<div class="flex flex-col gap-2.5 lg:grid lg:grid-cols-2 lg:gap-4 xl:grid-cols-3">
				{#each accounts as account (account.id)}
					<Card class={account.archived ? 'opacity-50' : ''}>
						<CardContent class="flex items-center justify-between pt-5">
							<div>
								<div class="text-sm font-semibold">{account.name}</div>
								<div class="text-xs text-muted capitalize">{account.kind} &middot; {account.currency}</div>
							</div>
							<div class="font-display text-base font-bold">{formatMoney(Number(account.balance), account.currency)}</div>
						</CardContent>
					</Card>
				{/each}
			</div>
		{/if}
	{/if}
</div>
