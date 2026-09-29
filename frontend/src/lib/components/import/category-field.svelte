<script lang="ts">
	// A category the person can type or pick from the ones already in use.
	// `suggestion` is what the server would apply if this stays empty (a
	// category every past entry with the same description agrees on), so it
	// is shown as a hint rather than filled in: leaving it alone accepts it.
	// Text is 16px on small screens so phones do not zoom in on focus.
	let {
		value = $bindable(''),
		suggestion = null,
		categories = [],
		disabled = false,
		onchange
	}: {
		value?: string;
		suggestion?: string | null;
		categories?: string[];
		disabled?: boolean;
		onchange?: (value: string) => void;
	} = $props();

	const uid = $props.id();
</script>

<div class="flex min-w-0 flex-col gap-1">
	<label for="category-{uid}" class="text-xs font-semibold text-muted">Category</label>
	<input
		id="category-{uid}"
		type="text"
		list="categories-{uid}"
		placeholder={suggestion ?? 'No category'}
		autocomplete="off"
		{disabled}
		class="min-h-11 w-full rounded-lg border border-border bg-page px-3 text-base placeholder:text-muted-2 focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none disabled:opacity-50 lg:min-h-10 lg:text-sm"
		bind:value
		onchange={() => onchange?.(value.trim())}
	/>
	<datalist id="categories-{uid}">
		{#each categories as category (category)}
			<option value={category}></option>
		{/each}
	</datalist>
	{#if value.trim() === ''}
		{#if suggestion}
			<p class="text-xs text-muted-2">
				Will be saved as <span class="font-semibold text-primary">{suggestion}</span>, from past entries.
			</p>
		{:else}
			<p class="text-xs text-muted-2">No category yet. You can add one later from Entries.</p>
		{/if}
	{/if}
</div>
