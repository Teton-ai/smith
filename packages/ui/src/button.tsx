import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";

export type ButtonTone =
	| "blue"
	| "purple"
	| "red"
	| "green"
	| "orange"
	| "gray";
export type ButtonVariant = "solid" | "soft" | "ghost";
export type ButtonSize = "sm" | "md";

// Full class strings per variant×tone so Tailwind's scanner can see them.
const BUTTON_STYLES: Record<ButtonVariant, Record<ButtonTone, string>> = {
	solid: {
		blue: "bg-blue-600 text-white hover:bg-blue-700",
		purple: "bg-purple-600 text-white hover:bg-purple-700",
		red: "bg-red-600 text-white hover:bg-red-700",
		green: "bg-green-600 text-white hover:bg-green-700",
		orange: "bg-orange-600 text-white hover:bg-orange-700",
		gray: "bg-gray-700 text-white hover:bg-gray-800",
	},
	soft: {
		blue: "bg-blue-50 text-blue-700 border border-blue-200 hover:bg-blue-100",
		purple:
			"bg-purple-50 text-purple-700 border border-purple-200 hover:bg-purple-100",
		red: "bg-red-50 text-red-700 border border-red-200 hover:bg-red-100",
		green:
			"bg-green-50 text-green-700 border border-green-200 hover:bg-green-100",
		orange:
			"bg-orange-50 text-orange-700 border border-orange-200 hover:bg-orange-100",
		gray: "bg-gray-50 text-gray-700 border border-gray-200 hover:bg-gray-100",
	},
	ghost: {
		blue: "text-blue-600 hover:bg-blue-50",
		purple: "text-purple-600 hover:bg-purple-50",
		red: "text-red-600 hover:bg-red-50",
		green: "text-green-600 hover:bg-green-50",
		orange: "text-orange-600 hover:bg-orange-50",
		gray: "text-gray-600 hover:bg-gray-100",
	},
};

const SIZE_STYLES: Record<ButtonSize, string> = {
	sm: "px-2.5 py-1.5 text-xs gap-1.5",
	md: "px-3 py-2 text-sm gap-2",
};

// Per-variant polish: subtle elevation that lifts on hover (skip for ghost).
const VARIANT_EXTRA: Record<ButtonVariant, string> = {
	solid: "shadow-sm hover:shadow-md",
	soft: "shadow-sm hover:shadow",
	ghost: "",
};

/**
 * Design-system button. Renders a `<button>`, a react-router `<Link>` (when
 * `to` is set), or an external `<a>` (when `href` is set).
 *
 * `variant="soft"` is the freshened, tinted look used for page actions;
 * `solid` is for primary CTAs; `ghost` for low-emphasis links.
 */
export function Button({
	variant = "soft",
	tone = "blue",
	size = "md",
	icon,
	loading = false,
	disabled = false,
	type = "button",
	title,
	onClick,
	to,
	href,
	target,
	className = "",
	children,
}: {
	variant?: ButtonVariant;
	tone?: ButtonTone;
	size?: ButtonSize;
	icon?: ReactNode;
	loading?: boolean;
	disabled?: boolean;
	type?: "button" | "submit";
	title?: string;
	onClick?: () => void;
	to?: string;
	href?: string;
	target?: string;
	className?: string;
	children?: ReactNode;
}) {
	const cls = `inline-flex items-center justify-center font-medium rounded-lg transition-colors duration-100 cursor-pointer active:scale-[.98] ${SIZE_STYLES[size]} ${VARIANT_EXTRA[variant]} ${BUTTON_STYLES[variant][tone]} ${className}`;
	const inner = (
		<>
			{loading ? <Loader2 className="w-4 h-4 animate-spin" /> : icon}
			{children}
		</>
	);

	if (to) {
		return (
			<Link to={to} className={cls} title={title}>
				{inner}
			</Link>
		);
	}
	if (href) {
		return (
			<a
				href={href}
				target={target}
				rel={target === "_blank" ? "noopener noreferrer" : undefined}
				className={cls}
				title={title}
			>
				{inner}
			</a>
		);
	}
	return (
		<button
			type={type}
			onClick={onClick}
			disabled={disabled || loading}
			title={title}
			className={`${cls} disabled:opacity-50 disabled:cursor-not-allowed`}
		>
			{inner}
		</button>
	);
}
