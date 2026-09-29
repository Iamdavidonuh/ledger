<script lang="ts">
	import { api, ApiError, type AccountWithBalance, type BankType, type ImportResult } from '$lib/api';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import { formatMoney } from '$lib/format';

	// Starts an import: which account, which bank's file format, and the file.
	// Nothing is saved until the statement passes its own balance check, and
	// an account with a review still open can't take another file, since
	// duplicates between two open imports would go unnoticed.
	let {
		accounts,
		blockedAccountIds,
		onuploaded
	}: {
		accounts: AccountWithBalance[];
		blockedAccountIds: string[];
		onuploaded: (result: ImportResult, accountId: string) => void;
	} = $props();

	const BANKS: { value: BankType; name: string; label: string; accept: string; extension: string; kind: string }[] = [
		{ value: 'BankA', name: 'BankA', label: 'BankA statement (CSV)', accept: '.csv,text/csv', extension: '.csv', kind: 'CSV' },
		{ value: 'BankB', name: 'BankB', label: 'BankB statement (PDF)', accept: '.pdf,application/pdf', extension: '.pdf', kind: 'PDF' }
	];

	const uid = $props.id();

	let chosenAccountId = $state('');
	let bank = $state<BankType>('BankA');
	let file = $state<File | null>(null);
	let fileInput = $state<HTMLInputElement | null>(null);
	let dragging = $state(false);
	let uploading = $state(false);
	let error = $state<string | null>(null);

	// Prefer an account that can take a file, but never leave the select
	// blank: with every account busy, the first one is shown with its notice.
	const accountId = $derived(
		chosenAccountId ||
			accounts.find((a) => !blockedAccountIds.includes(a.id))?.id ||
			accounts[0]?.id ||
			''
	);
	const account = $derived(accounts.find((a) => a.id === accountId));
	const blocked = $derived(blockedAccountIds.includes(accountId));
	const format = $derived(BANKS.find((b) => b.value === bank) ?? BANKS[0]);
	const ready = $derived(!!file && !!accountId && !blocked && !uploading);

	// The browser's own file picker filter (the `accept` attribute) is only
	// advisory: a person can still choose "All files" there, and a dropped
	// file never goes through it at all. BankA has no business accepting a
	// PDF and BankB has no business accepting a CSV, so this is checked
	// itself rather than trusted to the picker.
	function matchesFormat(candidate: File): boolean {
		return candidate.name.toLowerCase().endsWith(format.extension);
	}

	function pick(next: File | null | undefined) {
		if (!next) {
			file = null;
			error = null;
			return;
		}
		if (!matchesFormat(next)) {
			error = `${format.name} takes a ${format.kind} file. Choose a ${format.kind} file, or switch the format above.`;
			clearFile();
			return;
		}
		file = next;
		error = null;
	}

	// The browser keeps the chosen file in the input itself, and choosing the
	// same file again would not fire a change event, so clear it whenever the
	// selection is dropped.
	function clearFile() {
		file = null;
		if (fileInput) fileInput.value = '';
	}

	function drop(event: DragEvent) {
		event.preventDefault();
		dragging = false;
		pick(event.dataTransfer?.files[0]);
	}

	function leave(event: DragEvent) {
		// Moving over a child of the zone also fires dragleave on the zone.
		if (event.currentTarget instanceof Node && event.relatedTarget instanceof Node) {
			if (event.currentTarget.contains(event.relatedTarget)) return;
		}
		dragging = false;
	}

	function describe(err: unknown): string {
		if (!(err instanceof ApiError)) return 'Something went wrong uploading this file.';
		const { expected, actual } = err.body;
		if (err.status === 422 && typeof expected === 'string' && typeof actual === 'string') {
			const currency = account?.currency ?? '';
			return `This statement does not add up: it closes at ${formatMoney(Number(expected), currency)}, but its rows add up to ${formatMoney(Number(actual), currency)}. Nothing was saved.`;
		}
		// The importer's other errors (a file that won't parse, an unreadable
		// PDF, a settlement PDF instead of a statement) are written for logs,
		// as short lowercase fragments with no closing punctuation. Shown as
		// is, they read like an internal error rather than a plain sentence.
		return sentence(err.message);
	}

	function sentence(text: string): string {
		const trimmed = text.trim();
		if (trimmed === '') return trimmed;
		const withStop = /[.!?]$/.test(trimmed) ? trimmed : `${trimmed}.`;
		return withStop.charAt(0).toUpperCase() + withStop.slice(1);
	}

	async function upload(event: SubmitEvent) {
		event.preventDefault();
		if (!file || !ready) return;
		uploading = true;
		error = null;
		try {
			const result = await api.imports.upload(accountId, bank, file);
			clearFile();
			onuploaded(result, accountId);
		} catch (err) {
			error = describe(err);
			clearFile();
		} finally {
			uploading = false;
		}
	}
</script>

<Card class="@container p-5">
	<form class="flex flex-col gap-4" onsubmit={upload}>
		<div class="flex flex-col gap-1">
			<h2 class="font-display text-base font-bold">Import a statement</h2>
			<p class="text-sm text-muted">
				Every row waits for your review. Nothing reaches your ledger until you accept it.
			</p>
		</div>

		<div class="grid gap-3 @md:grid-cols-2">
			<div class="flex flex-col gap-1.5">
				<label for="account-{uid}" class="text-xs font-semibold text-muted">Into account</label>
				<select
					id="account-{uid}"
					class="min-h-11 rounded-xl border border-border bg-card px-3 text-base font-semibold focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:text-sm"
					value={accountId}
					onchange={(event) => (chosenAccountId = event.currentTarget.value)}
				>
					{#each accounts as candidate (candidate.id)}
						<option value={candidate.id}>
							{candidate.name} ({candidate.currency}){blockedAccountIds.includes(candidate.id)
								? ', review in progress'
								: ''}
						</option>
					{/each}
				</select>
			</div>

			<div class="flex flex-col gap-1.5">
				<label for="bank-{uid}" class="text-xs font-semibold text-muted">File format</label>
				<select
					id="bank-{uid}"
					class="min-h-11 rounded-xl border border-border bg-card px-3 text-base font-semibold focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-none lg:text-sm"
					bind:value={bank}
					onchange={clearFile}
				>
					{#each BANKS as option (option.value)}
						<option value={option.value}>{option.label}</option>
					{/each}
				</select>
			</div>
		</div>

		{#if blocked}
			<p class="rounded-xl border border-warn-border bg-warn-bg p-3 text-sm text-warn-foreground" role="status">
				This account has a review in progress. Finish or discard it before importing another file.
			</p>
		{/if}

		<label
			for="file-{uid}"
			ondragover={(event) => {
				event.preventDefault();
				dragging = true;
			}}
			ondragleave={leave}
			ondrop={drop}
			class="flex cursor-pointer flex-col items-center gap-2 rounded-2xl border border-dashed p-6 text-center transition-colors focus-within:ring-2 focus-within:ring-primary {dragging
				? 'border-primary bg-primary-soft'
				: 'border-border-dashed bg-page hover:bg-row-hover'}"
		>
			<span class="flex h-11 w-11 items-center justify-center rounded-full bg-primary-soft text-primary">
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"><path d="M12 3v12"></path><path d="M7 10l5 5 5-5"></path><path d="M4 19h16"></path></svg
				>
			</span>
			<span class="flex flex-col items-center gap-2" aria-live="polite">
				{#if file}
					<span class="max-w-full truncate text-sm font-semibold">{file.name}</span>
					<span class="text-xs text-muted-2">
						{(file.size / 1024).toFixed(0)} KB. Choose another file to replace it.
					</span>
				{:else}
					<span class="text-sm font-semibold">Drop a statement here, or choose a file</span>
					<span class="text-xs text-muted-2">
						{bank === 'BankA' ? 'A CSV export from BankA' : 'A PDF statement from BankB'}
					</span>
				{/if}
			</span>
			<input
				id="file-{uid}"
				bind:this={fileInput}
				type="file"
				accept={format.accept}
				class="sr-only"
				onchange={(event) => pick(event.currentTarget.files?.[0])}
			/>
		</label>

		{#if error}
			<p class="rounded-xl border border-warn-border bg-warn-bg p-3 text-sm text-warn-foreground" role="alert">
				{error}
			</p>
		{/if}

		<Button type="submit" disabled={!ready} class="w-full max-lg:h-12 @md:w-auto @md:self-end">
			{uploading ? 'Reading statement...' : 'Upload statement'}
		</Button>
	</form>
</Card>
