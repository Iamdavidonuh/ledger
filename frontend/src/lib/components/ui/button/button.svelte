<script lang="ts" module>
	import { tv, type VariantProps } from 'tailwind-variants';

	export const buttonVariants = tv({
		base: 'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full text-sm font-semibold transition-[filter] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:pointer-events-none disabled:opacity-50',
		variants: {
			variant: {
				default:
					'bg-primary text-primary-foreground shadow-[0_8px_20px_rgba(31,111,92,0.35)] hover:brightness-[1.06]',
				outline: 'border border-border bg-card text-foreground hover:bg-row-hover',
				ghost: 'text-foreground hover:bg-row-hover'
			},
			size: {
				default: 'h-11 px-5',
				sm: 'h-9 px-4 text-xs',
				icon: 'h-9 w-9 rounded-full p-0'
			}
		},
		defaultVariants: { variant: 'default', size: 'default' }
	});

	export type ButtonVariant = VariantProps<typeof buttonVariants>['variant'];
	export type ButtonSize = VariantProps<typeof buttonVariants>['size'];
</script>

<script lang="ts">
	import { cn } from '$lib/utils';
	import type { Snippet } from 'svelte';

	let {
		class: className = undefined,
		variant = 'default',
		size = 'default',
		href = undefined,
		type = 'button',
		children,
		...restProps
	}: {
		class?: string;
		variant?: ButtonVariant;
		size?: ButtonSize;
		href?: string;
		type?: 'button' | 'submit' | 'reset';
		children?: Snippet;
		[key: string]: unknown;
	} = $props();
</script>

{#if href}
	<a {href} class={cn(buttonVariants({ variant, size }), className)} {...restProps}>
		{@render children?.()}
	</a>
{:else}
	<button {type} class={cn(buttonVariants({ variant, size }), className)} {...restProps}>
		{@render children?.()}
	</button>
{/if}
