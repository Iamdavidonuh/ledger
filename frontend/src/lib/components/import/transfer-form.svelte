<script lang="ts">
	import type { AccountWithBalance, AcceptAsTransferRequest, NormalQueueRow } from '$lib/api';
	import { Button } from '$lib/components/ui/button';
	import { formatMoney } from '$lib/format';
	import { impliedRate } from '$lib/import/queue';

	// Turns a statement row into a transfer with another tracked account. The
	// row's own sign decides the direction, so the person only picks the other
	// side, and only supplies an amount when the two currencies differ.
	let {
		row,
		account,
		accounts,
		busy = false,
		onsubmit,
		oncancel
	}: {
		row: NormalQueueRow;
		account: AccountWithBalance;
		accounts: AccountWithBalance[];
		busy?: boolean;
		onsubmit: (request: AcceptAsTransferRequest) => void;
		oncancel: () => void;
	} = $props();

	const uid = $props.id();

	const others = $derived(accounts.filter((a) => a.id !== account.id && !a.archived));
	let otherId = $state('');
	let otherAmount = $state('');

	const other = $derived(others.find((a) => a.id === otherId));
	const crossCurrency = $derived(!!other && other.currency !== row.currency);
	const leaving = $derived(Number(row.amount) < 0);
	// A comma decimal ("12,50") is common; the API wants a period.
	const amountText = $derived(otherAmount.trim().replace(',', '.'));
	const amountValid = $derived(
		amountText !== '' && Number.isFinite(Number(amountText)) && Number(amountText) > 0
	);
	const rate = $derived(crossCurrency && amountValid ? impliedRate(row.amount, amountText) : null);
	const ready = $derived(!!other && (!crossCurrency || amountValid));

	function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!other || !ready) return;
		onsubmit({
			other_account_id: other.id,
			other_amount: crossCurrency ? amountText : undefined
		});
	}
</script>

<form class="flex flex-col gap-3 rounded-xl border border-border bg-page p-3" onsubmit={submit}>
	{#if others.length === 0}
		<p class="text-sm text-muted">Add another account to move money between accounts.</p>
		<div>
			<Button type="button" variant="ghost" size="sm" class="max-lg:h-11" onclick={oncancel}>Close</Button>
		</div>
	{:else}
		<p class="text-sm text-muted">
			{#if leaving}
				This money left <span class="font-semibold text-foreground">{account.name}</span> and arrived in
			{:else}
				This money arrived in <span class="font-semibold text-foreground">{account.name}</span> from
			{/if}
			another account of yours.
		</p>

		<div class="flex flex-col gap-1">
			<label for="other-{uid}" class="text-xs font-semibold text-muted">
				{leaving ? 'Arrived in' : 'Came from'}
			</label>
			<select
				id="other-{uid}"
				class="min-h-11 rounded-lg border border-border bg-card px-3 text-base focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10 lg:text-sm"
				bind:value={otherId}
			>
				<option value="" disabled>Choose an account</option>
				{#each others as candidate (candidate.id)}
					<option value={candidate.id}>{candidate.name} ({candidate.currency})</option>
				{/each}
			</select>
		</div>

		{#if crossCurrency && other}
			<div class="flex flex-col gap-1">
				<label for="amount-{uid}" class="text-xs font-semibold text-muted">
					Amount in {other.currency}
				</label>
				<input
					id="amount-{uid}"
					type="text"
					inputmode="decimal"
					placeholder="0.00"
					class="min-h-11 rounded-lg border border-border bg-card px-3 font-display text-base font-bold focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10 lg:text-sm"
					aria-invalid={otherAmount.trim() !== '' && !amountValid}
					bind:value={otherAmount}
				/>
				{#if otherAmount.trim() !== '' && !amountValid}
					<p class="text-xs text-warn-foreground">Enter a number greater than zero.</p>
				{:else if rate !== null}
					<p class="text-xs text-muted-2">
						That is 1 {row.currency} = {rate.toFixed(4)} {other.currency}. Only shown here, not saved.
					</p>
				{:else}
					<p class="text-xs text-muted-2">
						This statement shows {formatMoney(Math.abs(Number(row.amount)), row.currency)}. Enter what the other
						account shows.
					</p>
				{/if}
			</div>
		{/if}

		<div class="flex flex-wrap gap-3">
			<Button type="submit" size="sm" class="max-lg:h-11" disabled={busy || !ready}>Save as transfer</Button>
			<Button type="button" variant="ghost" size="sm" class="max-lg:h-11" disabled={busy} onclick={oncancel}>
				Cancel
			</Button>
		</div>
	{/if}
</form>
