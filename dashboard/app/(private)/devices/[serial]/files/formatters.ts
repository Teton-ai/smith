import type { FileKind } from "./useFileSession";

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

const KIND_PREFIX: Record<FileKind, string> = {
	Dir: "d",
	Symlink: "l",
	File: "-",
	Other: "?",
};

/** Render permission bits the way `ls -l` does, e.g. `drwxr-xr-x`. Operators
 *  read this shape instinctively; an octal number they have to decode. */
export function modeToRwx(mode: number, kind: FileKind): string {
	const bits = ["r", "w", "x"];
	let out = KIND_PREFIX[kind] ?? "?";

	for (let group = 2; group >= 0; group--) {
		const triad = (mode >> (group * 3)) & 0b111;
		for (let bit = 0; bit < 3; bit++) {
			out += triad & (0b100 >> bit) ? bits[bit] : "-";
		}
	}

	// setuid / setgid / sticky replace the matching execute bit, as in ls.
	const special = (mode >> 9) & 0b111;
	if (special & 0b100) out = replaceAt(out, 3, out[3] === "x" ? "s" : "S");
	if (special & 0b010) out = replaceAt(out, 6, out[6] === "x" ? "s" : "S");
	if (special & 0b001) out = replaceAt(out, 9, out[9] === "x" ? "t" : "T");

	return out;
}

function replaceAt(value: string, index: number, replacement: string): string {
	return value.slice(0, index) + replacement + value.slice(index + 1);
}

/** Unix seconds to a compact local timestamp. */
export function formatMtime(mtime: number | null): string {
	if (mtime === null || !Number.isFinite(mtime)) return "—";
	const date = new Date(mtime * 1000);
	if (Number.isNaN(date.getTime())) return "—";

	const now = new Date();
	const sameYear = date.getFullYear() === now.getFullYear();

	return date.toLocaleString(undefined, {
		month: "short",
		day: "numeric",
		year: sameYear ? undefined : "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

/** Split an absolute path into breadcrumb segments, root first. */
export function pathSegments(path: string): { label: string; key: string }[] {
	const parts = path.split("/").filter(Boolean);
	const segments = [{ label: "/", key: "/" }];

	let accumulated = "";
	for (const part of parts) {
		accumulated += `/${part}`;
		segments.push({ label: part, key: accumulated });
	}

	return segments;
}

/** Join a directory and an entry name without doubling the separator at root. */
export function joinPath(dir: string, name: string): string {
	return dir === "/" ? `/${name}` : `${dir}/${name}`;
}

/** Parent of an absolute path; root is its own parent. */
export function parentPath(path: string): string {
	const parts = path.split("/").filter(Boolean);
	parts.pop();
	return parts.length ? `/${parts.join("/")}` : "/";
}
