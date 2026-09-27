<script lang="ts">
	import ComingSoon from '$lib/components/coming-soon.svelte';
</script>

<div class="mx-auto flex h-dvh max-w-md flex-col bg-page">
	<div class="flex flex-none items-center gap-3 px-5 pt-[18px] pb-2">
		<a href="/" aria-label="Close" class="flex h-8 w-8 items-center justify-center">
			<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
				><path d="M6 6l12 12M18 6L6 18"></path></svg
			>
		</a>
		<div class="font-display text-[17px] font-bold">Add entry</div>
	</div>
	<div class="flex-1 px-5">
		<ComingSoon title="Add entry" />
	</div>
</div>
