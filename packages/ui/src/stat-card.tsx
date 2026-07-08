import type { ReactNode } from "react";
import type { IconComponent } from "./theme";

type StatTone = "neutral" | "green" | "blue" | "orange" | "red" | "purple";

const TONES: Record<StatTone, { badge: string; icon: string }> = {
	neutral: { badge: "bg-gray-100", icon: "text-gray-400" },
	green: { badge: "bg-green-100", icon: "text-green-600" },
	blue: { badge: "bg-blue-100", icon: "text-blue-600" },
	orange: { badge: "bg-orange-100", icon: "text-orange-600" },
	red: { badge: "bg-red-100", icon: "text-red-600" },
	purple: { badge: "bg-purple-100", icon: "text-purple-600" },
};

/**
 * The inner content of a stat tile: icon badge + label + big number.
 * Wrap in `Card`, `Link`, or `button` depending on whether it's clickable.
 */
export function StatCard({
	icon: Icon,
	label,
	value,
	tone = "neutral",
}: {
	icon: IconComponent;
	label: string;
	value: ReactNode;
	tone?: StatTone;
}) {
	const t = TONES[tone];
	return (
		<div className="flex items-center gap-4">
			<div
				className={`flex h-12 w-12 items-center justify-center rounded-xl ${t.badge}`}
			>
				<Icon className={`w-6 h-6 ${t.icon}`} />
			</div>
			<div>
				<p className="text-sm font-medium text-gray-500">{label}</p>
				<p className="text-3xl font-bold text-gray-900 tabular-nums">{value}</p>
			</div>
		</div>
	);
}
