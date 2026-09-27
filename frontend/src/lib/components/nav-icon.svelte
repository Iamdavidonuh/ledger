<script lang="ts">
	let { name }: { name: 'home' | 'entries' | 'import' | 'pots' | 'stats' | 'accounts' } = $props();
</script>

{#if name === 'home'}
	<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"
		><path d="M3 11l9-7 9 7"></path><path d="M5 10v9a1 1 0 001 1h4v-6h4v6h4a1 1 0 001-1v-9"></path></svg
	>
{:else if name === 'entries'}
	<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
		><line x1="4" y1="7" x2="20" y2="7"></line><line x1="4" y1="12" x2="20" y2="12"></line><line x1="4" y1="17" x2="14" y2="17"
		></line></svg
	>
{:else if name === 'import'}
	<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
		><path d="M12 3v12"></path><path d="M7 10l5 5 5-5"></path><path d="M4 19h16"></path></svg
	>
{:else if name === 'pots'}
	<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
		><path d="M4 10a4 4 0 014-4h6l4 4v6a2 2 0 01-2 2H8a4 4 0 01-4-4z"></path><circle cx="14.5" cy="12" r="1.2" fill="currentColor" stroke="none"
		></circle></svg
	>
{:else if name === 'stats'}
	<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
		><path d="M4 19V9"></path><path d="M11 19V5"></path><path d="M18 19v-7"></path></svg
	>
{:else if name === 'accounts'}
	<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
		><rect x="2" y="5" width="20" height="14" rx="2"></rect><path d="M2 9h20"></path></svg
	>
{/if}
