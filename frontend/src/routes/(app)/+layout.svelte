<script lang="ts">
	import { page } from '$app/state';
	import { Button } from '$lib/components/ui/button';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();

	const navItems = [
		{ href: '/', label: 'Home' },
		{ href: '/entries', label: 'Entries' },
		{ href: '/import', label: 'Import' },
		{ href: '/pots', label: 'Pots' },
		{ href: '/stats', label: 'Stats' }
	];

	function isActive(href: string) {
		return href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
	}
</script>

<div class="relative mx-auto flex h-dvh max-w-md flex-col overflow-hidden bg-page">
	<!-- Top bar -->
	<div class="flex flex-none items-center justify-between px-5 pt-5 pb-2">
		<div class="font-display text-xl font-bold tracking-tight">Ledger</div>
		<a
			href="/accounts"
			aria-label="Accounts"
			class="flex h-9 w-9 items-center justify-center rounded-full bg-border hover:opacity-70"
		>
			<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
				><rect x="2" y="5" width="20" height="14" rx="2"></rect><path d="M2 9h20"></path></svg
			>
		</a>
	</div>

	<!-- Scroll area -->
	<div class="flex-1 overflow-y-auto px-5 pt-2 pb-6">
		{@render children()}
	</div>

	{#if page.url.pathname === '/'}
		<Button href="/add-entry" class="absolute right-5 bottom-[92px] h-[52px] px-5">
			<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"
				><path d="M12 5v14M5 12h14"></path></svg
			>
			Add entry
		</Button>
	{/if}

	<!-- Bottom nav -->
	<div class="flex h-[72px] flex-none items-center border-t border-border bg-card px-2">
		{#each navItems as item (item.href)}
			<a
				href={item.href}
				class="flex flex-1 flex-col items-center gap-1 {isActive(item.href) ? 'text-primary' : 'text-muted-2'}"
			>
				{#if item.label === 'Home'}
					<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"
						><path d="M3 11l9-7 9 7"></path><path d="M5 10v9a1 1 0 001 1h4v-6h4v6h4a1 1 0 001-1v-9"></path></svg
					>
				{:else if item.label === 'Entries'}
					<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
						><line x1="4" y1="7" x2="20" y2="7"></line><line x1="4" y1="12" x2="20" y2="12"></line><line
							x1="4"
							y1="17"
							x2="14"
							y2="17"
						></line></svg
					>
				{:else if item.label === 'Import'}
					<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
						><path d="M12 3v12"></path><path d="M7 10l5 5 5-5"></path><path d="M4 19h16"></path></svg
					>
				{:else if item.label === 'Pots'}
					<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
						><path d="M4 10a4 4 0 014-4h6l4 4v6a2 2 0 01-2 2H8a4 4 0 01-4-4z"></path><circle
							cx="14.5"
							cy="12"
							r="1.2"
							fill="currentColor"
							stroke="none"
						></circle></svg
					>
				{:else}
					<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
						><path d="M4 19V9"></path><path d="M11 19V5"></path><path d="M18 19v-7"></path></svg
					>
				{/if}
				<span class="text-[10px] font-bold">{item.label}</span>
			</a>
		{/each}
	</div>
</div>
