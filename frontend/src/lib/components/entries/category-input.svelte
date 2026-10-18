<script lang="ts">
	// Category input with datalist autocomplete from past entries.
	// Text is 16px on small screens so phones do not zoom in on focus.
	let {
		value = $bindable(''),
		categories = [],
		disabled = false,
		id,
		class: cls = ''
	}: {
		value?: string;
		categories?: string[];
		disabled?: boolean;
		id?: string;
		class?: string;
	} = $props();

	const uid = $props.id();
	const inputId = $derived(id ?? `category-${uid}`);
</script>

<input
	id={inputId}
	type="text"
	list="categories-list-{uid}"
	placeholder="Groceries"
	autocomplete="off"
	{disabled}
	class={cls}
	bind:value
/>
<datalist id="categories-list-{uid}">
	{#each categories as category (category)}
		<option value={category}></option>
	{/each}
</datalist>
