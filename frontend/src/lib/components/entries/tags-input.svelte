<script lang="ts">
	// Tags input with datalist autocomplete. The datalist options are built
	// from the already-typed prefix so each segment autocompletes independently.
	// e.g. typing "tri" with known tags ["trip","camera"] offers "trip" but the
	// datalist value includes the already-committed tokens so accepting it does
	// not wipe them. Text is 16px on small screens so phones do not zoom in.
	let {
		value = $bindable(''),
		tags = [],
		disabled = false,
		id,
		class: cls = ''
	}: {
		value?: string;
		tags?: string[];
		disabled?: boolean;
		id?: string;
		class?: string;
	} = $props();

	const uid = $props.id();
	const inputId = $derived(id ?? `tags-${uid}`);

	// Split into committed tokens (all but last) and the current token being typed.
	const committed = $derived.by(() => {
		const parts = value.split(',');
		return parts.slice(0, -1).map((t) => t.trim()).filter(Boolean);
	});
	const currentToken = $derived(value.split(',').at(-1)?.trimStart() ?? '');

	// Prefix of the value up to the last comma (to prepend when building options).
	const prefix = $derived(
		committed.length > 0 ? committed.join(', ') + ', ' : ''
	);

	// Only show tags not already committed, filtered to the current token prefix.
	const suggestions = $derived(
		tags.filter(
			(t) =>
				!committed.includes(t) &&
				t.toLowerCase().startsWith(currentToken.toLowerCase())
		).map((t) => prefix + t)
	);
</script>

<input
	id={inputId}
	type="text"
	list="tags-list-{uid}"
	placeholder="trip, camera"
	autocomplete="off"
	{disabled}
	class={cls}
	bind:value
/>
<datalist id="tags-list-{uid}">
	{#each suggestions as s (s)}
		<option value={s}></option>
	{/each}
</datalist>
