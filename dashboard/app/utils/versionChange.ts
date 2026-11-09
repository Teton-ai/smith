import { compare, valid } from "semver";

export function versionChangeKind(
	previous: string,
	next: string,
	distributionChanged: boolean,
): "upgrade" | "rollback" | "distribution" | "change" {
	if (distributionChanged) return "distribution";
	const from = valid(previous);
	const to = valid(next);
	if (!from || !to) return "change";
	const direction = compare(to, from);
	return direction > 0 ? "upgrade" : direction < 0 ? "rollback" : "change";
}
