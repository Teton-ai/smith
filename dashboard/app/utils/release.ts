import { Cpu, HardDrive, Monitor, Package } from "lucide-react";
import type { Release } from "../api-client";

export function isStableRelease(release: Release): boolean {
	return !release.draft && !release.yanked && !release.release_candidate;
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
