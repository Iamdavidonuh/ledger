<script lang="ts">
	import { api, ApiError, type PotWithBalance } from '$lib/api';
	import { Button } from '$lib/components/ui/button';

	// Moves money between a pot and general savings (the part of your own
	// accounts' balance not yet claimed by any pot). A positive amount
	// takes from savings into the pot; a negative one puts it back. This
	// is a standalone allocation, not tied to any entry -- the backend
	// itself refuses one that would leave the pot, or savings, negative.
	let {
		pot,
		onallocated,
		oncancel
	}: {
		pot: PotWithBalance;
		onallocated: (updated: PotWithBalance) => void;
		oncancel: () => void;
	} = $props();

	const uid = $props.id();

	let amount = $state('');
	let date = $state(new Date().toISOString().slice(0, 10));
	let busy = $state(false);
	let error = $state<string | null>(null);

	// A comma decimal ("12,50") is common; the API wants a period.
	const amountText = $derived(amount.trim().replace(',', '.'));
	const amountValid = $derived(amountText !== '' && Number.isFinite(Number(amountText)) && Number(amountText) !== 0);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!amountValid) return;
		busy = true;
		error = null;
		try {
			const updated = await api.pots.allocate(pot.id, { amount: amountText, date });
			onallocated(updated);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not move that money.';
		} finally {
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-3 rounded-xl border border-border bg-page p-3" onsubmit={submit}>
	<div class="flex flex-col gap-1">
		<label for="amount-{uid}" class="text-xs font-semibold text-muted">
			Amount in {pot.currency}
		</label>
		<input
			id="amount-{uid}"
			type="text"
			inputmode="decimal"
			placeholder="50.00"
			class="min-h-11 rounded-lg border border-border bg-card px-3 font-display text-base font-bold focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10 lg:text-sm"
			aria-invalid={amount.trim() !== '' && !amountValid}
			bind:value={amount}
		/>
		<p class="text-xs text-muted-2">
			Positive moves money from general savings into {pot.name}. Negative puts it back.
		</p>
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
		<Button type="submit" size="sm" class="max-lg:h-11" disabled={busy || !amountValid}>
			{busy ? 'Moving...' : 'Move money'}
		</Button>
		<Button type="button" variant="ghost" size="sm" class="max-lg:h-11" disabled={busy} onclick={oncancel}>
			Cancel
		</Button>
	</div>
</form>
