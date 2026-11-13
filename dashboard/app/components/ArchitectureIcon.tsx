import { BADGE_COLORS, type BadgeVariant } from "@teton/smith-ui";
import { Cpu, HardDrive, Monitor, Package } from "lucide-react";

/** Badge colour for a distribution's CPU architecture. */
export const architectureVariant = (architecture: string): BadgeVariant => {
	switch (architecture.toLowerCase()) {
		case "x86_64":
		case "amd64":
			return "blue";
		case "arm64":
		case "aarch64":
			return "green";
		case "armv7":
		case "arm":
			return "purple";
		default:
			return "gray";
	}
};

/** A distribution's architecture as a tinted icon tile, as on its page. */
export function ArchitectureIcon({
	architecture,
	size = "md",
}: {
	architecture: string;
	size?: "sm" | "md";
}) {
	const icon = size === "sm" ? "w-3.5 h-3.5" : "w-5 h-5";
	const Icon = (() => {
		switch (architecture.toLowerCase()) {
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
	})();
	return (
		<span
			title={architecture}
			className={`inline-flex shrink-0 rounded-lg ${size === "sm" ? "p-1" : "p-2"} ${BADGE_COLORS[architectureVariant(architecture)]}`}
		>
			<Icon className={icon} />
		</span>
	);
}
