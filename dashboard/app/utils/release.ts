import type { Release } from "../api-client";

export function isStableRelease(release: Release): boolean {
	return !release.draft && !release.yanked && !release.release_candidate;
}
