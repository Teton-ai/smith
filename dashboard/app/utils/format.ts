const UNITS = ["B", "KB", "MB", "GB", "TB"];

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
