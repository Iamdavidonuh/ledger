export type NavItemId = 'home' | 'entries' | 'import' | 'pots' | 'stats';

export const NAV_ITEMS: { id: NavItemId; href: string; label: string }[] = [
	{ id: 'home', href: '/', label: 'Home' },
	{ id: 'entries', href: '/entries', label: 'Entries' },
	{ id: 'import', href: '/import', label: 'Import' },
	{ id: 'pots', href: '/pots', label: 'Pots' },
	{ id: 'stats', href: '/stats', label: 'Stats' }
];
