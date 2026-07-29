import { ChevronDown } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import {
	Button,
	type ButtonSize,
	type ButtonTone,
	type ButtonVariant,
} from "./button";

export interface DropdownItem {
	label: ReactNode;
	/** Secondary line under the label — good for the detail a tooltip carried. */
	description?: ReactNode;
	icon?: ReactNode;
	/** Tints the icon with the tone the action would have as a standalone button. */
	tone?: ButtonTone;
	onClick?: () => void;
	/** Leaves the menu open after selecting — for items that swap to an
	 *  in-place confirmation (e.g. "Copied!") instead of navigating away. */
	keepOpen?: boolean;
	/** Renders the item as an external link instead of a button. */
	href?: string;
	target?: string;
	disabled?: boolean;
}

// Full class strings so Tailwind's scanner can see them.
const ITEM_ICON_TONE: Record<ButtonTone, string> = {
	blue: "text-blue-600",
	purple: "text-purple-600",
	red: "text-red-600",
	green: "text-green-600",
	orange: "text-orange-600",
	gray: "text-gray-500",
};

const ITEM_CLS =
	"w-full flex items-start gap-3 px-3 py-2 text-left transition-colors hover:bg-gray-50 cursor-pointer";

/**
 * A button that opens a menu of actions — for collapsing a row of related
 * buttons into one control. Closes on outside click, Escape, or item select.
 */
export function DropdownMenu({
	label,
	items,
	icon,
	variant = "soft",
	tone = "gray",
	size = "md",
	align = "right",
	className = "",
}: {
	label: ReactNode;
	items: DropdownItem[];
	icon?: ReactNode;
	variant?: ButtonVariant;
	tone?: ButtonTone;
	size?: ButtonSize;
	/** Which edge of the trigger the menu is anchored to. */
	align?: "left" | "right";
	className?: string;
}) {
	const [open, setOpen] = useState(false);
	const containerRef = useRef<HTMLDivElement>(null);

	useEffect(() => {
		if (!open) return;
		const onPointerDown = (event: MouseEvent) => {
			if (!containerRef.current?.contains(event.target as Node)) setOpen(false);
		};
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") setOpen(false);
		};
		document.addEventListener("mousedown", onPointerDown);
		document.addEventListener("keydown", onKeyDown);
		return () => {
			document.removeEventListener("mousedown", onPointerDown);
			document.removeEventListener("keydown", onKeyDown);
		};
	}, [open]);

	return (
		<div className={`relative ${className}`} ref={containerRef}>
			<Button
				variant={variant}
				tone={tone}
				size={size}
				icon={icon}
				onClick={() => setOpen((o) => !o)}
			>
				{label}
				<ChevronDown
					className={`w-4 h-4 transition-transform duration-200 ${
						open ? "rotate-180" : ""
					}`}
				/>
			</Button>

			{open && (
				<div
					className={`absolute ${align === "right" ? "right-0" : "left-0"} mt-2 w-64 py-1 bg-white rounded-lg shadow-lg border border-gray-200 z-50 animate-dropdown-in`}
				>
					{items.map((item, index) => {
						const iconTone = ITEM_ICON_TONE[item.tone ?? "gray"];
						const inner = (
							<>
								{item.icon && (
									<span
										className={`flex items-center gap-1 mt-0.5 flex-shrink-0 ${iconTone}`}
									>
										{item.icon}
									</span>
								)}
								<span className="min-w-0">
									<span className="block text-sm font-medium text-gray-900">
										{item.label}
									</span>
									{item.description && (
										<span className="block text-xs text-gray-500 mt-0.5 break-all">
											{item.description}
										</span>
									)}
								</span>
							</>
						);

						if (item.href) {
							return (
								<a
									key={index}
									href={item.href}
									target={item.target}
									rel={
										item.target === "_blank" ? "noopener noreferrer" : undefined
									}
									className={ITEM_CLS}
									onClick={() => setOpen(false)}
								>
									{inner}
								</a>
							);
						}
						return (
							<button
								key={index}
								type="button"
								disabled={item.disabled}
								onClick={() => {
									if (!item.keepOpen) setOpen(false);
									item.onClick?.();
								}}
								className={`${ITEM_CLS} disabled:opacity-50 disabled:cursor-not-allowed`}
							>
								{inner}
							</button>
						);
					})}
				</div>
			)}
		</div>
	);
}
