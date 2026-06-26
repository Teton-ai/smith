import type { ReactNode } from "react";
import { Link } from "react-router";
import type { IconComponent, SectionTheme } from "./theme";

/**
 * Base card surface — the standard white panel used across the app.
 * Compose with `className` for layout (e.g. `overflow-hidden flex flex-col`).
 */
export function Card({
	className = "",
	children,
}: {
	className?: string;
	children: ReactNode;
}) {
	return (
		<div
			className={`bg-white rounded-xl border border-gray-200/80 shadow-sm ${className}`}
		>
			{children}
		</div>
	);
}

/**
 * A detail-page section: same colored header bar as `SectionCard`, but with a
 * padded freeform body (`bodyClassName`) instead of a divided list — for
 * panels like System Information, Network, Location.
 */
export function Panel({
	icon: Icon,
	title,
	theme,
	count,
	actions,
	bodyClassName = "p-5",
	className = "",
	children,
}: {
	icon?: IconComponent;
	title: ReactNode;
	theme: SectionTheme;
	count?: number;
	actions?: ReactNode;
	bodyClassName?: string;
	className?: string;
	children: ReactNode;
}) {
	return (
		<Card className={`overflow-hidden ${className}`}>
			<div
				className={`px-4 py-3 flex items-center justify-between border-b border-black/5 ${theme.header}`}
			>
				<h4 className="text-sm font-semibold flex items-center">
					{Icon && <Icon className="w-4 h-4 mr-2" />}
					{title}
				</h4>
				{(actions || count !== undefined) && (
					<div className="flex items-center gap-2">
						{actions}
						{count !== undefined && (
							<span
								className={`text-xs font-semibold px-2 py-0.5 rounded-full ${theme.badge}`}
							>
								{count}
							</span>
						)}
					</div>
				)}
			</div>
			<div className={bodyClassName}>{children}</div>
		</Card>
	);
}

/**
 * Card with a colored, titled header bar — for grouped lists.
 * Right side shows a count pill (when `count` is set) and/or `actions`.
 */
export function SectionCard({
	icon: Icon,
	title,
	count,
	theme,
	actions,
	footer,
	children,
}: {
	icon?: IconComponent;
	title: ReactNode;
	count?: number;
	theme: SectionTheme;
	actions?: ReactNode;
	footer?: ReactNode;
	children: ReactNode;
}) {
	return (
		<Card className="overflow-hidden flex flex-col">
			<div
				className={`px-4 py-3 flex items-center justify-between border-b border-black/5 ${theme.header}`}
			>
				<h4 className="text-sm font-semibold flex items-center">
					{Icon && <Icon className="w-4 h-4 mr-2" />}
					{title}
				</h4>
				<div className="flex items-center gap-2">
					{actions}
					{count !== undefined && (
						<span
							className={`text-xs font-semibold px-2 py-0.5 rounded-full ${theme.badge}`}
						>
							{count}
						</span>
					)}
				</div>
			</div>
			<div className="divide-y divide-gray-100">{children}</div>
			{footer}
		</Card>
	);
}

/**
 * A single clickable list row (Link or button), with the standard
 * hover + transition. Left/right content is composed by the caller.
 */
export function ListRow({
	to,
	onClick,
	hover = "hover:bg-gray-50",
	className = "",
	children,
}: {
	to?: string;
	onClick?: () => void;
	hover?: string;
	className?: string;
	children: ReactNode;
}) {
	const interactive = Boolean(to || onClick);
	const base = `flex items-center justify-between px-4 py-3 transition-colors ${hover} ${interactive ? "cursor-pointer" : ""} ${className}`;
	if (to) {
		return (
			<Link to={to} className={base}>
				{children}
			</Link>
		);
	}
	if (onClick) {
		return (
			<button
				type="button"
				onClick={onClick}
				className={`w-full text-left ${base}`}
			>
				{children}
			</button>
		);
	}
	// Static (non-interactive) row — still shows hover feedback.
	return <div className={base}>{children}</div>;
}

/** "View all N items →" footer link for truncated lists. */
export function ViewAllFooter({
	to,
	count,
	noun = "items",
}: {
	to: string;
	count: number;
	noun?: string;
}) {
	return (
		<Link
			to={to}
			className="block px-4 py-2.5 text-sm font-medium text-blue-600 hover:text-blue-700 hover:bg-gray-50 transition-colors"
		>
			View all {count} {noun} →
		</Link>
	);
}
