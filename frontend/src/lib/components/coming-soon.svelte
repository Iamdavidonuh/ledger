<script lang="ts">
	let { title }: { title: string } = $props();
</script>

<div class="flex h-full flex-col items-center justify-center gap-2 text-center">
	<div class="font-display text-lg font-bold">{title}</div>
	<p class="max-w-[240px] text-sm text-muted">This screen isn't wired up yet.</p>
</div>
