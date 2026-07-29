import { ChevronRight, MoreHorizontal } from "lucide-react";
import { useState } from "react";

export interface BreadcrumbSegment {
	label: string;
	/** Distinguishes segments that repeat (e.g. `/var/lib/var`). */
	key: string;
}

/**
 * Path breadcrumb with a clickable trail. The last segment is the current
 * location and renders as static text; the rest call `onNavigate` with their
 * index.
 *
 * Deep paths collapse in the middle rather than wrapping or truncating the
 * end — the first and last few segments are the ones that orient you, and the
 * hidden middle stays reachable through the overflow menu.
 */
export function Breadcrumbs({
	segments,
	onNavigate,
	maxVisible = 5,
	className = "",
}: {
	segments: BreadcrumbSegment[];
	onNavigate: (index: number) => void;
	maxVisible?: number;
	className?: string;
}) {
	const [expanded, setExpanded] = useState(false);

	const collapsed = !expanded && segments.length > maxVisible;
	// Keep the root plus the tail: those anchor where you are and where you
	// came from.
	const leading = collapsed ? segments.slice(0, 1) : [];
	const hiddenCount = collapsed ? segments.length - maxVisible : 0;
	const trailing = collapsed ? segments.slice(1 + hiddenCount) : segments;
	const trailingOffset = collapsed ? 1 + hiddenCount : 0;

	const crumb = (segment: BreadcrumbSegment, index: number) => {
		const isLast = index === segments.length - 1;
		return isLast ? (
			<span
				key={segment.key}
				aria-current="page"
				className="px-1.5 py-0.5 font-medium text-gray-900 truncate max-w-[16rem]"
			>
				{segment.label}
			</span>
		) : (
			<button
				key={segment.key}
				type="button"
				onClick={() => onNavigate(index)}
				className="px-1.5 py-0.5 rounded text-gray-500 hover:text-gray-900 hover:bg-gray-100 transition-colors cursor-pointer truncate max-w-[12rem]"
			>
				{segment.label}
			</button>
		);
	};

	return (
		<nav
			aria-label="Breadcrumb"
			className={`flex items-center gap-0.5 text-sm min-w-0 overflow-hidden ${className}`}
		>
			{leading.map((segment, index) => (
				<span key={segment.key} className="flex items-center gap-0.5 min-w-0">
					{crumb(segment, index)}
					<ChevronRight className="w-3.5 h-3.5 text-gray-300 flex-shrink-0" />
				</span>
			))}

			{collapsed && (
				<span className="flex items-center gap-0.5 flex-shrink-0">
					<button
						type="button"
						onClick={() => setExpanded(true)}
						title={`Show ${hiddenCount} hidden ${hiddenCount === 1 ? "folder" : "folders"}`}
						className="px-1 py-0.5 rounded text-gray-400 hover:text-gray-700 hover:bg-gray-100 transition-colors cursor-pointer"
					>
						<MoreHorizontal className="w-4 h-4" />
					</button>
					<ChevronRight className="w-3.5 h-3.5 text-gray-300" />
				</span>
			)}

			{trailing.map((segment, index) => {
				const absolute = trailingOffset + index;
				return (
					<span key={segment.key} className="flex items-center gap-0.5 min-w-0">
						{crumb(segment, absolute)}
						{absolute < segments.length - 1 && (
							<ChevronRight className="w-3.5 h-3.5 text-gray-300 flex-shrink-0" />
						)}
					</span>
				);
			})}
		</nav>
	);
}
