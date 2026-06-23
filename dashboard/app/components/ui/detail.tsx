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

export interface SideNavItem {
	label: string;
	to: string;
	icon?: IconComponent;
	active?: boolean;
}

/** Vertical settings sub-nav: a stacked list of links with an icon, label and
 *  an animated left accent bar on the active item — matching the main sidebar's
 *  look (blue active state, icon lift on hover). The active item renders as
 *  static text; the rest are router links. */
export function SideNav({ items }: { items: SideNavItem[] }) {
	return (
		<nav className="space-y-1">
			{items.map((item) => {
				const Icon = item.icon;
				const cls = `group/side relative flex items-center gap-3 h-10 rounded-md px-3 text-sm transition-all duration-200 cursor-pointer ${
					item.active
						? "bg-blue-50 text-blue-700 font-medium"
						: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
				}`;
				const inner = (
					<>
						<span
							className={`absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-r-full bg-blue-600 transition-all duration-300 ease-out ${
								item.active ? "opacity-100 scale-y-100" : "opacity-0 scale-y-0"
							}`}
						/>
						{Icon && (
							<Icon className="w-[18px] h-[18px] shrink-0 transition-transform duration-200 group-hover/side:scale-110" />
						)}
						<span className="truncate">{item.label}</span>
					</>
				);
				return item.active ? (
					<span key={item.to} className={cls} aria-current="page">
						{inner}
					</span>
				) : (
					<Link key={item.to} to={item.to} className={cls}>
						{inner}
					</Link>
				);
			})}
		</nav>
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
