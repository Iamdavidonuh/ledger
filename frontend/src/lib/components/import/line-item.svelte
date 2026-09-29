<script lang="ts">
	import { formatClock, formatDay, formatSignedMoney } from '$lib/format';

	// One money line in a compare block: what it is, when, and how much. The
	// same shape serves a statement row and an entry already in the ledger, so
	// two of them sit side by side and read the same. The description wraps
	// rather than truncating: it is the main way to tell a real duplicate from
	// a coincidence.
	let {
		label,
		description,
		date,
		time,
		amount,
		currency
	}: {
		label: string;
		description: string;
		date: string;
		time: string | null;
		amount: string;
		currency: string;
	} = $props();

	const clock = $derived(formatClock(time));
</script>

<div class="flex min-w-0 flex-col gap-0.5 rounded-xl bg-page p-3">
	<p class="text-[11px] font-semibold text-muted-2">{label}</p>
	<p class="line-clamp-2 text-[13px] font-semibold break-words" title={description}>{description}</p>
	<p class="text-xs text-muted">{formatDay(date)}{clock ? `, ${clock}` : ''}</p>
	<p class="font-display text-[13px] font-bold">{formatSignedMoney(amount, currency)}</p>
</div>
