const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** A tag is a label stored with an empty value and written as just its key. */
export function formatLabel(key: string, value: string): string {
	return value === "" ? key : `${key}=${value}`;
}

/** For display only: filters must keep using the plain key. */
export function labelName(key: string): string {
	return key === "pinned" ? "📍 pinned" : key;
}

export function displayLabel(key: string, value: string): string {
	return formatLabel(labelName(key), value);
}

/** Pinned first, then keys in a deterministic, case-sensitive order. */
export function labelEntries(
	labels: Record<string, string> | null | undefined,
): [string, string][] {
	return Object.entries(labels ?? {}).sort(
		([a], [b]) =>
			Number(b === "pinned") - Number(a === "pinned") ||
			(a < b ? -1 : a > b ? 1 : 0),
	);
}

/** Sizes are shown at three significant figures — enough to compare two files
 *  at a glance without implying byte-level precision. */
export function humanBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes < 0) return "—";
	if (bytes === 0) return "0 B";

	let value = bytes;
	let unit = 0;
	while (value >= 1000 && unit < UNITS.length - 1) {
		value /= 1000;
		unit += 1;
	}

	const decimals = value >= 100 || unit === 0 ? 0 : value >= 10 ? 1 : 2;
	return `${value.toFixed(decimals)} ${UNITS[unit]}`;
}
