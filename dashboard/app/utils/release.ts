import { isAxiosError } from "axios";
import { Cpu, HardDrive, Monitor, Package } from "lucide-react";
import type { Release } from "../api-client";

export function isStableRelease(release: Release): boolean {
	return !release.draft && !release.yanked && !release.release_candidate;
}

/** Releases devices on `from` can be rolled back to, newest first. */
export function rollbackCandidates(
	releases: Release[],
	from: Release,
): Release[] {
	return releases
		.filter((r) => r.id !== from.id && isStableRelease(r))
		.sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at));
}

/**
 * The fleet's latest release when that is still a valid target, otherwise
 * the newest stable release older than `from`: the last version known to be
 * good before it, not an untested newer one.
 */
export function defaultRollbackTarget(
	candidates: Release[],
	from: Release,
	latest?: Release,
): Release | undefined {
	if (latest && candidates.some((r) => r.id === latest.id)) return latest;
	const fromCreated = Date.parse(from.created_at);
	return (
		candidates.find((r) => Date.parse(r.created_at) < fromCreated) ??
		candidates[0]
	);
}

/** The API returns plain-text reasons on 4xx; prefer them over axios' generic message. */
export function requestErrorMessage(error: unknown): string {
	if (isAxiosError(error)) {
		const data = error.response?.data;
		if (typeof data === "string" && data.trim()) return data;
		return error.message;
	}
	return error instanceof Error ? error.message : "Unknown error";
}

/**
 * Icon standing in for the hardware a build targets: x86 boxes are screens,
 * 64-bit ARM boards a CPU, older ARM a drive. Shared so a device, its
 * distribution and its releases are never drawn with different icons.
 */
export function architectureIcon(architecture?: string) {
	switch (architecture?.toLowerCase()) {
		case "x86_64":
		case "amd64":
			return Monitor;
		case "arm64":
		case "aarch64":
			return Cpu;
		case "armv7":
		case "arm":
			return HardDrive;
		default:
			return Package;
	}
}
