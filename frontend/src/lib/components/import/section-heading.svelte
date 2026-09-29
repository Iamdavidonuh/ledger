<script lang="ts">
	let { title, count, hint }: { title: string; count?: number; hint?: string } = $props();
</script>

<div class="flex items-baseline justify-between gap-3">
	<h2 class="text-[13px] font-semibold tracking-wide text-muted uppercase">
		{title}{#if count !== undefined}
			<span class="font-display normal-case"> &middot; {count}</span>
		{/if}
	</h2>
	{#if hint}
		<p class="text-right text-xs text-muted-2">{hint}</p>
	{/if}
</div>
