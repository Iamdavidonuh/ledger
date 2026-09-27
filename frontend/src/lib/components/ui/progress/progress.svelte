<script lang="ts">
	import { cn } from '$lib/utils';

	let {
		value = 0,
		max = 100,
		class: className = undefined
	}: { value?: number; max?: number; class?: string } = $props();

	let pct = $derived(max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0);
</script>

<div
	class={cn('h-2 w-full overflow-hidden rounded-full bg-app', className)}
	role="progressbar"
	aria-valuenow={value}
	aria-valuemin={0}
	aria-valuemax={max}
>
	<div class="h-full rounded-full bg-primary transition-[width]" style={`width: ${pct}%`}></div>
</div>
