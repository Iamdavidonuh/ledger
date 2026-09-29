import type { ImportQueueRow, NormalQueueRow, RevertedCandidateQueueRow } from '$lib/api';

// The review screen shows a queue in three groups, in the order a person
// should deal with them: rows that resemble something already recorded, bank
// reversals, and rows that are clean. Mirrors the server's own split (a
// Normal row with any match is suspicious; bulk accept only ever takes the
// clean ones).
export interface QueueGroups {
	needsDecision: NormalQueueRow[];
	candidates: RevertedCandidateQueueRow[];
	clean: NormalQueueRow[];
}

export function groupQueue(rows: ImportQueueRow[]): QueueGroups {
	const groups: QueueGroups = { needsDecision: [], candidates: [], clean: [] };
	for (const row of rows) {
		if (row.kind === 'RevertedCandidate') {
			groups.candidates.push(row);
		} else if (row.suspicious) {
			groups.needsDecision.push(row);
		} else {
			groups.clean.push(row);
		}
	}
	return groups;
}

// Accept all is refused by the server while any suspicious row or any bank
// reversal is still in the queue. Computing the same gate here lets the
// button say why it is off instead of failing after the click.
export interface BulkGate {
	blocked: boolean;
	suspicious: number;
	candidates: number;
}

export function bulkGate(groups: QueueGroups): BulkGate {
	const suspicious = groups.needsDecision.length;
	const candidates = groups.candidates.length;
	return { blocked: suspicious + candidates > 0, suspicious, candidates };
}

function plural(count: number, one: string, many: string): string {
	return `${count} ${count === 1 ? one : many}`;
}

export function gateMessage(gate: BulkGate): string | null {
	if (!gate.blocked) return null;
	const parts: string[] = [];
	if (gate.suspicious > 0) parts.push(plural(gate.suspicious, 'possible duplicate', 'possible duplicates'));
	if (gate.candidates > 0) parts.push(plural(gate.candidates, 'bank reversal', 'bank reversals'));
	return `Decide on ${parts.join(' and ')} first.`;
}

// The rate a cross-currency transfer implies: what arrived divided by what
// left, shown back to the person and never stored. Null until both amounts
// are usable numbers.
export function impliedRate(known: string, other: string): number | null {
	const sent = Math.abs(Number(known));
	const received = Number(other);
	if (!Number.isFinite(sent) || !Number.isFinite(received) || sent === 0 || received <= 0) {
		return null;
	}
	return received / sent;
}
