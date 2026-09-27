<script lang="ts">
	import { page } from '$app/state';
	import NavIcon from './nav-icon.svelte';
	import { NAV_ITEMS } from '$lib/nav';

	function isActive(href: string) {
		return href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
	}
</script>

<div class="hidden w-[232px] flex-none flex-col border-r border-border bg-card p-3.5 lg:flex">
	<div class="mb-6 px-2.5 font-display text-lg font-bold">Ledger</div>

	{#each NAV_ITEMS as item (item.id)}
		<a
			href={item.href}
			class="mb-0.5 flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-semibold {isActive(item.href)
				? 'bg-row-hover text-primary'
				: 'text-muted hover:bg-page'}"
		>
			<NavIcon name={item.id} />
			{item.label}
		</a>
	{/each}

	<div class="flex-1"></div>

	<a
		href="/accounts"
		class="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-semibold {isActive('/accounts')
			? 'bg-row-hover text-primary'
			: 'text-muted hover:bg-page'}"
	>
		<NavIcon name="accounts" />
		Accounts
	</a>
</div>
