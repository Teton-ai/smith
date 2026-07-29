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

const ALERT_TONES = {
	red: { accent: "border-l-red-500", chip: "bg-red-50 text-red-600" },
	amber: { accent: "border-l-amber-500", chip: "bg-amber-50 text-amber-700" },
};

export type AlertTone = keyof typeof ALERT_TONES;

/**
 * Attention banner for a problem the page wants to lead with: colored left
 * accent, headline plus severity chip, one line of detail, optional action on
 * the right. Callers render it conditionally — it has no "all clear" state.
 */
export function AlertBanner({
	tone = "red",
	title,
	severity,
	action,
	children,
}: {
	tone?: AlertTone;
	title: ReactNode;
	severity?: string;
	action?: ReactNode;
	children: ReactNode;
}) {
	const { accent, chip } = ALERT_TONES[tone];

	return (
		<Card className={`border-l-4 ${accent} px-5 py-4`}>
			<div className="flex items-start justify-between gap-4">
				{/* Grows so body content that wants the width — bars, timelines — gets
				    it, rather than shrinking to the length of the text above it. */}
				<div className="min-w-0 flex-1">
					<div className="flex items-center gap-2.5 flex-wrap">
						<h4 className="text-base font-semibold text-gray-900">{title}</h4>
						{severity && (
							<span
								className={`text-[11px] font-bold uppercase tracking-wide px-1.5 py-0.5 rounded ${chip}`}
							>
								{severity}
							</span>
						)}
					</div>
					<div className="mt-1 text-sm text-gray-600">{children}</div>
				</div>
				{action && <div className="shrink-0 text-sm">{action}</div>}
			</div>
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
