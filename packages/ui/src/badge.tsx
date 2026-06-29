import { X } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";

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
 *  Renders just the key when no `value` is given. Pass `onClick` to make the
 *  whole chip a filter toggle; pass `onRemove` to add a trailing × button
 *  (e.g. for an applied filter). `active` highlights it as the applied filter. */
export function LabelChip({
	name,
	value,
	onClick,
	onRemove,
	active = false,
	title,
}: {
	name: string;
	value?: string;
	onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
	onRemove?: (e: MouseEvent<HTMLButtonElement>) => void;
	active?: boolean;
	title?: string;
}) {
	const inner = (
		<>
			<span
				className={`px-2 py-1 font-medium ${
					active ? "text-blue-700 bg-blue-100" : "text-gray-500 bg-gray-50"
				}`}
			>
				{name}
			</span>
			{value !== undefined && value !== "" && (
				<span
					className={`px-2 py-1 font-mono border-l ${
						active
							? "text-blue-700 bg-blue-100 border-blue-200"
							: "text-gray-900 bg-white border-gray-200"
					}`}
				>
					{value}
				</span>
			)}
		</>
	);
	const base = `inline-flex items-center overflow-hidden rounded-md border text-xs ${
		active ? "border-blue-300" : "border-gray-200"
	}`;
	// Trailing × button: chip stays a static container so we don't nest buttons.
	if (onRemove) {
		return (
			<span className={base}>
				{inner}
				<button
					type="button"
					onClick={onRemove}
					title={title}
					aria-label={`Remove ${value !== undefined ? `${name}=${value}` : name}`}
					className={`flex items-center px-1.5 py-1 border-l cursor-pointer transition-colors ${
						active
							? "bg-blue-100 border-blue-200 text-blue-500 hover:bg-blue-200 hover:text-blue-700"
							: "border-gray-200 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
					}`}
				>
					<X className="w-3 h-3" />
				</button>
			</span>
		);
	}
	if (onClick) {
		return (
			<button
				type="button"
				onClick={onClick}
				title={title}
				className={`${base} cursor-pointer transition-colors ${
					active ? "" : "hover:border-gray-300"
				}`}
			>
				{inner}
			</button>
		);
	}
	return <span className={base}>{inner}</span>;
}
