<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, ApiError, type AccountWithBalance, type ImportResult, type ImportSummary } from '$lib/api';
	import { Card, CardContent } from '$lib/components/ui/card';
	import ImportHistory, { type OpenImport } from '$lib/components/import/import-history.svelte';
	import ImportReview from '$lib/components/import/import-review.svelte';
	import UploadCard from '$lib/components/import/upload-card.svelte';

	type OpenImportInAccount = OpenImport & { accountId: string };

	let accounts = $state<AccountWithBalance[]>([]);
	let openImports = $state<OpenImportInAccount[]>([]);
	let pastImports = $state<ImportSummary[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Which import to have open comes from the URL (?review=<id>), not local
	// state, so the browser's own Back button leaves a review the same way it
	// leaves anywhere else: back to whatever was on screen before it, here the
	// import list, rather than out of the app entirely.
	const reviewId = $derived(page.url.searchParams.get('review'));

	// Known from the list load, or as soon as a file finishes uploading;
	// filled in on demand below for a deep link straight to a review (a
	// bookmark, or a page reload while reviewing).
	let importAccountId = $state<Record<string, string>>({});
	let importIntro = $state<Record<string, string>>({});

	const reviewingAccount = $derived.by(() => {
		const accountId = reviewId ? importAccountId[reviewId] : undefined;
		return accounts.find((account) => account.id === accountId);
	});
	const blockedAccountIds = $derived(openImports.map((item) => item.accountId));

	async function load() {
		loading = true;
		error = null;
		try {
			const [nextAccounts, summaries] = await Promise.all([api.accounts.list(), api.imports.list()]);
			const open = summaries.filter((summary) => !summary.completed);
			// The list only carries a summary, so which account each open review
			// belongs to comes from the full import record.
			const details = await Promise.all(open.map((summary) => api.imports.get(summary.id)));
			accounts = nextAccounts;
			openImports = open.map((summary, index) => {
				const accountId = details[index].account_id;
				return {
					summary,
					accountId,
					accountName: nextAccounts.find((account) => account.id === accountId)?.name ?? 'Unknown account'
				};
			});
			importAccountId = {
				...importAccountId,
				...Object.fromEntries(openImports.map((item) => [item.summary.id, item.accountId]))
			};
			pastImports = summaries
				.filter((summary) => summary.completed)
				.sort((a, b) => b.uploaded_at.localeCompare(a.uploaded_at));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Something went wrong loading your imports.';
		} finally {
			loading = false;
		}
	}

	onMount(load);

	// Reload the list whenever the person leaves a review (finishing one, or
	// pressing Back), so it reflects whatever just changed.
	let previousReviewId: string | null = null;
	$effect(() => {
		const current = reviewId;
		if (previousReviewId !== null && current === null) load();
		previousReviewId = current;
	});

	// A review reached directly, not through this page's own state (a
	// reload, or a bookmarked link): look up its account once.
	$effect(() => {
		const id = reviewId;
		if (!id || loading || importAccountId[id]) return;
		api.imports
			.get(id)
			.then((imported) => {
				importAccountId = { ...importAccountId, [id]: imported.account_id };
			})
			.catch(() => {
				// Not a real import id; there is nothing to review.
				goto('/import', { replaceState: true });
			});
	});

	function review(importId: string) {
		goto(`/import?review=${importId}`);
	}

	function plural(count: number, one: string, many: string): string {
		return `${count} ${count === 1 ? one : many}`;
	}

	function uploaded(result: ImportResult, accountId: string) {
		importAccountId = { ...importAccountId, [result.import_id]: accountId };
		if (result.total_rows > 0) {
			importIntro = {
				...importIntro,
				[result.import_id]: `${plural(result.total_rows, 'row', 'rows')} read: ${plural(result.suspicious_count, 'possible duplicate', 'possible duplicates')} and ${plural(result.reverted_candidate_count, 'bank reversal', 'bank reversals')}.`
			};
		}
		goto(`/import?review=${result.import_id}`);
	}

	function leaveReview() {
		goto('/import');
	}
</script>

<div
	role="presentation"
	ondragover={(event) => event.preventDefault()}
	ondrop={(event) => event.preventDefault()}
>
	{#if reviewId && reviewingAccount}
		<div class="lg:max-w-5xl">
			<ImportReview
				importId={reviewId}
				account={reviewingAccount}
				{accounts}
				intro={importIntro[reviewId]}
				onexit={leaveReview}
			/>
		</div>
	{:else if reviewId}
		<p class="text-sm text-muted" role="status">Loading this review...</p>
	{:else}
		<div class="flex flex-col gap-5 lg:max-w-5xl">
			<h1 class="font-display text-xl font-bold">Import</h1>

			{#if loading}
				<p class="text-sm text-muted" role="status">Loading...</p>
			{:else if error}
				<Card class="border-warn-border bg-warn-bg">
					<CardContent class="pt-5 text-sm text-warn-foreground" role="alert">{error}</CardContent>
				</Card>
			{:else if accounts.length === 0}
				<p class="text-sm text-muted">Open an account first, then import a statement into it.</p>
			{:else}
				<div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,22rem)] lg:items-start">
					<UploadCard {accounts} {blockedAccountIds} onuploaded={uploaded} />
					<div class="flex flex-col gap-6">
						<ImportHistory {openImports} {pastImports} onreview={review} />
					</div>
				</div>
			{/if}
		</div>
	{/if}
</div>
