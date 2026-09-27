export function startOfCurrentMonth(): Date {
	const date = new Date();
	date.setDate(1);
	date.setHours(0, 0, 0, 0);
	return date;
}
