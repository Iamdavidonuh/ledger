<script lang="ts">
	import { api, ApiError, type Account, type AccountKind } from '$lib/api';
	import { Button } from '$lib/components/ui/button';

	let {
		oncreated,
		oncancel
	}: {
		oncreated: (account: Account) => void;
		oncancel: () => void;
	} = $props();

	const uid = $props.id();

	const KINDS: { value: AccountKind; label: string; hint: string }[] = [
		{ value: 'own', label: 'Own account', hint: 'A bank, cash, or savings account you hold' },
		{ value: 'outside', label: 'Outside account', hint: 'Real, but not tracked day to day' },
		{ value: 'person', label: 'A person', hint: 'Money owed to, or by, someone' },
		{ value: 'investment', label: 'Investment', hint: 'Value you update yourself, e.g. an ETF' }
	];

	let name = $state('');
	let currency = $state('');
	let kind = $state<AccountKind>('own');
	let openingBalance = $state('0');
	let busy = $state(false);
	let error = $state<string | null>(null);

	// Currency::new on the backend accepts 2-10 alphanumeric characters,
	// uppercased; matched here so a bad code is caught before the request.
	const currencyText = $derived(currency.trim().toUpperCase());
	const currencyValid = $derived(/^[A-Z0-9]{2,10}$/.test(currencyText));
	// Only sign/number checked, not a minimum: a Person account can
	// legitimately open already owing money, same as opening_balance
	// itself has no sign restriction on the backend.
	const balanceText = $derived(openingBalance.trim().replace(',', '.'));
	const balanceValid = $derived(balanceText !== '' && Number.isFinite(Number(balanceText)));
	const ready = $derived(name.trim() !== '' && currencyValid && balanceValid);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!ready) return;
		busy = true;
		error = null;
		try {
			const account = await api.accounts.create({
				name: name.trim(),
				currency: currencyText,
				kind,
				opening_balance: balanceText
			});
			oncreated(account);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not create this account.';
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-4 rounded-xl border border-border bg-card p-4" onsubmit={submit}>
	<div class="grid gap-3 @md:grid-cols-2">
		<div class="flex flex-col gap-1">
			<label for="name-{uid}" class="text-xs font-semibold text-muted">Name</label>
			<input
				id="name-{uid}"
				type="text"
				placeholder="Checking"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				bind:value={name}
				required
			/>
		</div>

		<div class="flex flex-col gap-1">
			<label for="currency-{uid}" class="text-xs font-semibold text-muted">Currency</label>
			<input
				id="currency-{uid}"
				type="text"
				placeholder="EUR"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm uppercase focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				aria-invalid={currency.trim() !== '' && !currencyValid}
				bind:value={currency}
				required
			/>
		</div>

		<div class="flex flex-col gap-1">
			<label for="balance-{uid}" class="text-xs font-semibold text-muted">Opening balance</label>
			<input
				id="balance-{uid}"
				type="text"
				inputmode="decimal"
				placeholder="0.00"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				aria-invalid={openingBalance.trim() !== '' && !balanceValid}
				bind:value={openingBalance}
				required
			/>
		</div>

		<div class="flex flex-col gap-1">
			<label for="kind-{uid}" class="text-xs font-semibold text-muted">Kind</label>
			<select
				id="kind-{uid}"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				bind:value={kind}
			>
				{#each KINDS as option (option.value)}
					<option value={option.value}>{option.label}</option>
				{/each}
			</select>
		</div>
	</div>

	<p class="text-xs text-muted-2">
		{KINDS.find((k) => k.value === kind)?.hint}
	</p>

	{#if error}
		<p class="text-xs text-warn-foreground">{error}</p>
	{/if}

	<div class="flex flex-wrap gap-3">
		<Button type="submit" size="sm" class="max-lg:h-11" disabled={busy || !ready}>
			{busy ? 'Creating...' : 'Create account'}
		</Button>
		<Button type="button" variant="ghost" size="sm" class="max-lg:h-11" disabled={busy} onclick={oncancel}>
			Cancel
		</Button>
	</div>
</form>
