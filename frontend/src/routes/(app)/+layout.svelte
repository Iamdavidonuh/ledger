<script lang="ts">
	import { page } from '$app/state';
	import { Button } from '$lib/components/ui/button';
	import NavIcon from '$lib/components/nav-icon.svelte';
	import NavSidebar from '$lib/components/nav-sidebar.svelte';
	import { NAV_ITEMS } from '$lib/nav';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();

	function isActive(href: string) {
		return href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
	}
</script>

<div class="flex h-dvh bg-page">
	<NavSidebar />

	<!-- Mobile app column: full-width phone shell below lg, plain flexed
	     main column (sidebar already provides the app frame) at lg+. -->
	<div class="relative mx-auto flex h-dvh w-full max-w-md flex-col overflow-hidden lg:mx-0 lg:max-w-none">
		<!-- Top bar: mobile only, the sidebar carries branding + accounts on desktop. -->
		<div class="flex flex-none items-center justify-between px-5 pt-5 pb-2 lg:hidden">
			<div class="font-display text-xl font-bold tracking-tight">Ledger</div>
			<a
				href="/accounts"
				aria-label="Accounts"
				class="flex h-9 w-9 items-center justify-center rounded-full bg-border hover:opacity-70"
			>
				<NavIcon name="accounts" />
			</a>
		</div>

		<!-- Scroll area. Extra bottom padding on Home only, mobile only: the
		     FAB there sits at bottom-[92px] with its own 52px height, so
		     content needs to clear roughly 144px, not just the bottom nav's
		     height. -->
		<div
			class="flex-1 overflow-y-auto px-5 pt-2 pb-6 lg:px-10 lg:py-8 {page.url.pathname === '/' ? 'pb-40 lg:pb-8' : ''}"
		>
			{@render children()}
		</div>

		{#if page.url.pathname === '/'}
			<Button href="/add-entry" class="absolute right-5 bottom-[92px] h-[52px] px-5 lg:hidden">
				<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"
					><path d="M12 5v14M5 12h14"></path></svg
				>
				Add entry
			</Button>
		{/if}

		<!-- Bottom nav: mobile only. -->
		<div class="flex h-[72px] flex-none items-center border-t border-border bg-card px-2 lg:hidden">
			{#each NAV_ITEMS as item (item.id)}
				<a
					href={item.href}
					class="flex flex-1 flex-col items-center gap-1 {isActive(item.href) ? 'text-primary' : 'text-muted-2'}"
				>
					<NavIcon name={item.id} />
					<span class="text-[10px] font-bold">{item.label}</span>
				</a>
			{/each}
		</div>
	</div>
</div>
