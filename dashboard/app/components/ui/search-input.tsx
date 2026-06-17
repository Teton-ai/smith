import { Search } from "lucide-react";

/** Search text input with a leading magnifying-glass icon. Size via `className`
 *  on the wrapper (defaults to full width). */
export function SearchInput({
	value,
	onChange,
	placeholder = "Search...",
	className = "w-full",
}: {
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
	className?: string;
}) {
	return (
		<div className={`relative ${className}`}>
			<Search className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 w-4 h-4 pointer-events-none" />
			<input
				type="text"
				value={value}
				onChange={(e) => onChange(e.target.value)}
				placeholder={placeholder}
				className="w-full pl-10 pr-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
			/>
		</div>
	);
}
