import { ArrowLeft } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";
import type { IconComponent } from "./theme";

/** "← Back" navigation link for detail pages. Pass `to` for a route or
 *  `onClick` (e.g. `() => navigate(-1)`) for history-based back. */
export function BackLink({
	to,
	onClick,
	children,
}: {
	to?: string;
	onClick?: () => void;
	children: ReactNode;
}) {
	const cls =
		"inline-flex items-center gap-2 text-sm font-medium text-gray-600 hover:text-gray-900 transition-colors cursor-pointer";
	const inner = (
		<>
			<ArrowLeft className="w-4 h-4" />
			<span>{children}</span>
		</>
	);
	if (to) {
		return (
			<Link to={to} className={cls}>
				{inner}
			</Link>
		);
	}
	return (
		<button type="button" onClick={onClick} className={cls}>
			{inner}
		</button>
	);
}

export interface TabItem {
	label: string;
	to?: string;
	active?: boolean;
}

/** Underlined tab bar used across the device detail subpages. The active tab
 *  renders as static text; the rest are router links. */
export function TabNav({ items }: { items: TabItem[] }) {
	return (
		<div className="border-b border-gray-200">
			<nav className="-mb-px flex space-x-8">
				{items.map((item) => {
					const cls = item.active
						? "py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
						: "py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer";
					return item.to && !item.active ? (
						<Link key={item.label} to={item.to} className={cls}>
							{item.label}
						</Link>
					) : (
						<span key={item.label} className={cls}>
							{item.label}
						</span>
					);
				})}
			</nav>
		</div>
	);
}

/** A label/value row for detail panels (`label` on the left, `children` on
 *  the right). Optional leading `icon` next to the label. */
export function InfoRow({
	label,
	icon: Icon,
	iconClassName = "text-gray-400",
	children,
}: {
	label: ReactNode;
	icon?: IconComponent;
	iconClassName?: string;
	children: ReactNode;
}) {
	return (
		<div className="flex items-center justify-between gap-4">
			<span className="text-gray-700 flex items-center flex-shrink-0">
				{Icon && <Icon className={`w-4 h-4 mr-2 ${iconClassName}`} />}
				{label}
			</span>
			<span className="text-sm text-gray-900 text-right min-w-0">
				{children}
			</span>
		</div>
	);
}
