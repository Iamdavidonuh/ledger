export function startOfCurrentMonth(): Date {
	const date = new Date();
	date.setDate(1);
	date.setHours(0, 0, 0, 0);
	return date;
}

// "2026-03-01" as local midnight, not `new Date("2026-03-01")`'s UTC
// midnight -- comparing that directly against a local Date (like
// startOfCurrentMonth's) silently shifts entries near a month boundary on
// any machine not on UTC. Invalid input returns an Invalid Date rather
// than throwing, same as the raw constructor.
export function parseLocalDate(date: string): Date {
	return new Date(`${date}T00:00:00`);
}
