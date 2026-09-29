<script lang="ts">
	import { untrack } from 'svelte';
	import { api, ApiError, type Pot } from '$lib/api';
	import { Button } from '$lib/components/ui/button';

	// currencies is every currency an own account already holds -- a pot
	// only makes sense in a currency there is actually savings in, since
	// its balance is drawn from that currency's general savings.
	let {
		currencies,
		oncreated,
		oncancel
	}: {
		currencies: string[];
		oncreated: (pot: Pot) => void;
		oncancel: () => void;
	} = $props();

	const uid = $props.id();

	let name = $state('');
	// Read once on mount: `currencies` doesn't change under an open form,
	// and this is just the initial pick, not a value that should track it.
	let currency = $state(untrack(() => currencies[0] ?? ''));
	let target = $state('');
	let priority = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	const targetText = $derived(target.trim().replace(',', '.'));
	const targetValid = $derived(targetText === '' || (Number.isFinite(Number(targetText)) && Number(targetText) > 0));
	const priorityValid = $derived(priority.trim() === '' || Number.isInteger(Number(priority)));
	const ready = $derived(name.trim() !== '' && !!currency && targetValid && priorityValid);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!ready) return;
		busy = true;
		error = null;
		try {
			const pot = await api.pots.open({
				name: name.trim(),
				currency,
				target: targetText === '' ? null : targetText,
				priority: priority.trim() === '' ? null : Number(priority)
			});
			oncreated(pot);
			name = '';
			target = '';
			priority = '';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Could not create that pot.';
		} finally {
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-3 rounded-xl border border-border bg-card p-4" onsubmit={submit}>
	<div class="grid gap-3 @md:grid-cols-2">
		<div class="flex flex-col gap-1">
			<label for="name-{uid}" class="text-xs font-semibold text-muted">Name</label>
			<input
				id="name-{uid}"
				type="text"
				placeholder="Trip fund"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				bind:value={name}
				required
			/>
		</div>

		<div class="flex flex-col gap-1">
			<label for="currency-{uid}" class="text-xs font-semibold text-muted">Currency</label>
			<select
				id="currency-{uid}"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				bind:value={currency}
			>
				{#each currencies as code (code)}
					<option value={code}>{code}</option>
				{/each}
			</select>
		</div>

		<div class="flex flex-col gap-1">
			<label for="target-{uid}" class="text-xs font-semibold text-muted">Target (optional)</label>
			<input
				id="target-{uid}"
				type="text"
				inputmode="decimal"
				placeholder="2000.00"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				aria-invalid={!targetValid}
				bind:value={target}
			/>
		</div>

		<div class="flex flex-col gap-1">
			<label for="priority-{uid}" class="text-xs font-semibold text-muted">Priority (optional)</label>
			<input
				id="priority-{uid}"
				type="text"
				inputmode="numeric"
				placeholder="1"
				class="min-h-11 rounded-lg border border-border bg-page px-3 text-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:min-h-10"
				aria-invalid={!priorityValid}
				bind:value={priority}
			/>
		</div>
	</div>

	{#if error}
		<p class="text-xs text-warn-foreground">{error}</p>
	{/if}

	<div class="flex flex-wrap gap-3">
		<Button type="submit" size="sm" class="max-lg:h-11" disabled={busy || !ready}>
			{busy ? 'Creating...' : 'Create pot'}
		</Button>
		<Button type="button" variant="ghost" size="sm" class="max-lg:h-11" disabled={busy} onclick={oncancel}>
			Cancel
		</Button>
	</div>
</form>
