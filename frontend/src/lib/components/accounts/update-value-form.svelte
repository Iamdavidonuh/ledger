<script lang="ts">
	import { api, ApiError, type AccountWithBalance, type Valuation } from '$lib/api';
	import { Button } from '$lib/components/ui/button';

	// Only meaningful for an Investment account: its balance isn't driven
	// by entries day to day, so this is how its value actually changes.
	let {
		account,
		onupdated,
		oncancel
	}: {
		account: AccountWithBalance;
		onupdated: (valuation: Valuation) => void;
		oncancel: () => void;
	} = $props();

	const uid = $props.id();

	const currentValue = $derived(account.current_value ?? account.balance);

	let newValue = $state('');
	let category = $state('Market update');
	let date = $state(new Date().toISOString().slice(0, 10));
	let busy = $state(false);
	let error = $state<string | null>(null);

	const valueText = $derived(newValue.trim().replace(',', '.'));
	const valueValid = $derived(valueText !== '' && Number.isFinite(Number(valueText)));
	const ready = $derived(valueValid && category.trim() !== '');

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!ready) return;
		busy = true;
		error = null;
		try {
			const valuation = await api.accounts.updateCurrentValue(account.id, {
				new_value: valueText,
				category: category.trim(),
				date
			});
			onupdated(valuation);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not update this value.';
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-3 rounded-xl border border-border bg-page p-3" onsubmit={submit}>
	<div class="flex flex-col gap-1">
		<label for="value-{uid}" class="text-xs font-semibold text-muted">
			New value in {account.currency} (currently {currentValue})
		</label>
		<input
			id="value-{uid}"
			type="text"
			inputmode="decimal"
			placeholder={currentValue}
			class="min-h-11 rounded-lg border border-border bg-card px-3 font-display text-base font-bold focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10 lg:text-sm"
			aria-invalid={newValue.trim() !== '' && !valueValid}
			bind:value={newValue}
		/>
	</div>

	<div class="flex flex-col gap-1">
		<label for="category-{uid}" class="text-xs font-semibold text-muted">Reason</label>
		<input
			id="category-{uid}"
			type="text"
			placeholder="Market update"
			class="min-h-11 rounded-lg border border-border bg-card px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
			bind:value={category}
		/>
	</div>

	<div class="flex flex-col gap-1">
		<label for="date-{uid}" class="text-xs font-semibold text-muted">Date</label>
		<input
			id="date-{uid}"
			type="date"
			class="min-h-11 rounded-lg border border-border bg-card px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
			bind:value={date}
			required
		/>
	</div>

	{#if error}
		<p class="text-xs text-warn-foreground">{error}</p>
	{/if}

	<div class="flex flex-wrap gap-3">
		<Button type="submit" size="sm" class="max-lg:h-11" disabled={busy || !ready}>
			{busy ? 'Saving...' : 'Save value'}
		</Button>
		<Button type="button" variant="ghost" size="sm" class="max-lg:h-11" disabled={busy} onclick={oncancel}>
			Cancel
		</Button>
	</div>
</form>
