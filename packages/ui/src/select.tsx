import { Check, ChevronDown } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";

/** Native single-select styled to match the form fields elsewhere. Children are
 *  the `<option>` / `<optgroup>` elements. Size via `className` (defaults to
 *  auto width so it can sit inline next to a label). */
export function Select({
	value,
	onChange,
	children,
	id,
	className = "w-auto",
	disabled = false,
}: {
	value: string;
	onChange: (value: string) => void;
	children: ReactNode;
	id?: string;
	className?: string;
	disabled?: boolean;
}) {
	return (
		<select
			id={id}
			value={value}
			onChange={(e) => onChange(e.target.value)}
			disabled={disabled}
			className={`px-3 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 text-sm disabled:opacity-50 ${className}`}
		>
			{children}
		</select>
	);
}

export interface SelectMenuOption {
	value: string;
	label: string;
	/** Secondary line under the label — for the detail a tooltip would carry. */
	description?: ReactNode;
}

/**
 * Value picker with `DropdownMenu`'s popover chrome. Use it instead of `Select`
 * where a native `<select>`'s platform styling sits next to design-system
 * buttons and inputs; the trigger shows the selected option's label. Closes on
 * outside click, Escape, or selection; Arrow keys move the highlight.
 */
export function SelectMenu({
	value,
	onChange,
	options,
	ariaLabel,
	align = "left",
	className = "",
	menuClassName = "w-56",
}: {
	value: string;
	onChange: (value: string) => void;
	options: SelectMenuOption[];
	ariaLabel: string;
	/** Which edge of the trigger the menu is anchored to. */
	align?: "left" | "right";
	className?: string;
	menuClassName?: string;
}) {
	const [open, setOpen] = useState(false);
	const [highlighted, setHighlighted] = useState<number | null>(null);
	const containerRef = useRef<HTMLDivElement>(null);

	const selectedIndex = options.findIndex((o) => o.value === value);
	const selected = selectedIndex >= 0 ? options[selectedIndex] : undefined;

	useEffect(() => {
		if (!open) return;
		const onPointerDown = (event: MouseEvent) => {
			if (!containerRef.current?.contains(event.target as Node)) setOpen(false);
		};
		document.addEventListener("mousedown", onPointerDown);
		return () => document.removeEventListener("mousedown", onPointerDown);
	}, [open]);

	function commit(index: number) {
		const option = options[index];
		if (!option) return;
		onChange(option.value);
		setOpen(false);
	}

	function onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
		if (event.key === "Escape") {
			setOpen(false);
			return;
		}
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			if (!open) {
				setOpen(true);
				setHighlighted(selectedIndex >= 0 ? selectedIndex : 0);
				return;
			}
			setHighlighted((prev) => {
				const from = prev ?? selectedIndex;
				const next = from + (event.key === "ArrowDown" ? 1 : -1);
				return Math.max(0, Math.min(next, options.length - 1));
			});
			return;
		}
		if (open && event.key === "Enter" && highlighted != null) {
			event.preventDefault();
			commit(highlighted);
		}
	}

	return (
		<div
			className={`relative ${className}`}
			ref={containerRef}
			onKeyDown={onKeyDown}
		>
			<button
				type="button"
				aria-label={ariaLabel}
				aria-haspopup="listbox"
				aria-expanded={open}
				onClick={() => {
					setOpen((o) => !o);
					setHighlighted(selectedIndex >= 0 ? selectedIndex : 0);
				}}
				className="w-full inline-flex items-center justify-between gap-2 px-3 py-2 text-sm font-medium rounded-lg border border-gray-200 bg-gray-50 text-gray-700 shadow-sm transition-colors duration-100 cursor-pointer hover:bg-gray-100 hover:shadow"
			>
				<span className="truncate">{selected?.label ?? value}</span>
				<ChevronDown
					className={`w-4 h-4 flex-shrink-0 text-gray-400 transition-transform duration-200 ${
						open ? "rotate-180" : ""
					}`}
				/>
			</button>

			{open && (
				<div
					role="listbox"
					aria-label={ariaLabel}
					tabIndex={-1}
					className={`absolute ${align === "right" ? "right-0" : "left-0"} mt-2 ${menuClassName} max-h-72 overflow-y-auto py-1 bg-white rounded-lg shadow-lg border border-gray-200 z-50 animate-dropdown-in`}
				>
					{options.map((option, index) => {
						const isSelected = option.value === value;
						return (
							<button
								key={option.value}
								type="button"
								role="option"
								aria-selected={isSelected}
								onMouseEnter={() => setHighlighted(index)}
								onClick={() => commit(index)}
								className={`w-full flex items-start gap-2 px-3 py-2 text-left transition-colors cursor-pointer ${
									highlighted === index ? "bg-gray-50" : ""
								}`}
							>
								<Check
									className={`w-4 h-4 mt-0.5 flex-shrink-0 text-blue-600 ${
										isSelected ? "" : "invisible"
									}`}
								/>
								<span className="min-w-0">
									<span
										className={`block text-sm ${isSelected ? "font-medium text-gray-900" : "text-gray-700"}`}
									>
										{option.label}
									</span>
									{option.description && (
										<span className="block text-xs text-gray-500 mt-0.5">
											{option.description}
										</span>
									)}
								</span>
							</button>
						);
					})}
				</div>
			)}
		</div>
	);
}
