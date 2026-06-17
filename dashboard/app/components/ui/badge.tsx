import type { ReactNode } from "react";

export type BadgeVariant =
	| "gray"
	| "blue"
	| "green"
	| "yellow"
	| "red"
	| "purple"
	| "orange";

/** Color classes per variant — exported so non-badge elements (icon tiles,
 *  etc.) can reuse the same palette. */
export const BADGE_COLORS: Record<BadgeVariant, string> = {
	gray: "bg-gray-100 text-gray-700",
	blue: "bg-blue-100 text-blue-700",
	green: "bg-green-100 text-green-700",
	yellow: "bg-yellow-100 text-yellow-700",
	red: "bg-red-100 text-red-700",
	purple: "bg-purple-100 text-purple-700",
	orange: "bg-orange-100 text-orange-700",
};

/** Small status/label pill. */
export function Badge({
	variant = "gray",
	pill = false,
	className = "",
	children,
}: {
	variant?: BadgeVariant;
	/** Fully rounded (rounded-full) vs slightly rounded (rounded). */
	pill?: boolean;
	className?: string;
	children: ReactNode;
}) {
	return (
		<span
			className={`px-1.5 py-0.5 text-xs font-medium ${pill ? "rounded-full" : "rounded"} ${BADGE_COLORS[variant]} ${className}`}
		>
			{children}
		</span>
	);
}

/** Two-tone key/value chip for device labels (key in gray, value in mono).
 *  Renders just the key when no `value` is given. */
export function LabelChip({ name, value }: { name: string; value?: string }) {
	return (
		<span className="inline-flex items-center overflow-hidden rounded-md border border-gray-200 text-xs">
			<span className="px-2 py-1 font-medium text-gray-500 bg-gray-50">
				{name}
			</span>
			{value !== undefined && value !== "" && (
				<span className="px-2 py-1 font-mono text-gray-900 bg-white border-l border-gray-200">
					{value}
				</span>
			)}
		</span>
	);
}
