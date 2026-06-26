import { Search } from "lucide-react";
import { useEffect, useRef } from "react";

/** Search text input with a leading magnifying-glass icon. Size via `className`
 *  on the wrapper (defaults to full width). Set `slashToFocus` to focus the
 *  input when the user presses "/" anywhere outside a field. */
export function SearchInput({
	value,
	onChange,
	placeholder = "Search...",
	className = "w-full",
	slashToFocus = false,
}: {
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
	className?: string;
	slashToFocus?: boolean;
}) {
	const inputRef = useRef<HTMLInputElement>(null);

	useEffect(() => {
		if (!slashToFocus) return;
		function handleKeyDown(e: KeyboardEvent) {
			if (e.key !== "/") return;
			if (e.ctrlKey || e.metaKey || e.altKey) return;
			const target = e.target as HTMLElement;
			if (
				target.tagName === "INPUT" ||
				target.tagName === "TEXTAREA" ||
				target.tagName === "SELECT" ||
				target.isContentEditable
			)
				return;
			e.preventDefault();
			inputRef.current?.focus();
		}
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [slashToFocus]);

	return (
		<div className={`relative ${className}`}>
			<Search className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 w-4 h-4 pointer-events-none" />
			<input
				ref={inputRef}
				type="text"
				value={value}
				onChange={(e) => onChange(e.target.value)}
				placeholder={placeholder}
				className="w-full pl-10 pr-10 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
			/>
			{slashToFocus && !value && (
				<kbd className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 text-xs border border-gray-300 rounded px-1 py-0.5 font-mono leading-none pointer-events-none">
					/
				</kbd>
			)}
		</div>
	);
}
