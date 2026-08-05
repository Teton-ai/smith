import type { ReactNode } from "react";

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
