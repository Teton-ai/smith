import type { ReactNode } from "react";

/**
 * Standard scrollable page wrapper used across the app.
 * `space-y-6` between direct children; override with `className` when a page
 * needs a non-default layout (e.g. full-height flex columns).
 */
export function PageContainer({
	className = "space-y-6",
	children,
}: {
	className?: string;
	children: ReactNode;
}) {
	return (
		<div
			className={`flex-1 overflow-y-auto px-4 sm:px-6 lg:px-8 py-6 ${className}`}
		>
			{children}
		</div>
	);
}
