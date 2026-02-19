"use client";

import { Loader2 } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";

const variantStyles = {
	primary: "bg-blue-600 hover:bg-blue-700 text-white",
	secondary: "bg-gray-100 hover:bg-gray-200 text-gray-700",
	danger: "bg-red-600 hover:bg-red-700 text-white",
	success: "bg-green-600 hover:bg-green-700 text-white",
	warning: "bg-amber-600 hover:bg-amber-700 text-white",
	purple: "bg-purple-600 hover:bg-purple-700 text-white",
	ghost: "text-blue-600 hover:text-blue-800",
} as const;

type Variant = keyof typeof variantStyles;

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
	variant?: Variant;
	loading?: boolean;
	icon?: ReactNode;
}

export function Button({
	variant = "primary",
	loading = false,
	icon,
	children,
	disabled,
	className = "",
	...props
}: ButtonProps) {
	const base =
		variant === "ghost"
			? "text-sm cursor-pointer"
			: "px-3 py-2 text-sm rounded-md cursor-pointer";

	return (
		<button
			disabled={disabled || loading}
			className={`${base} ${variantStyles[variant]} disabled:opacity-50 disabled:cursor-not-allowed ${className}`}
			{...props}
		>
			{loading ? (
				<span className="flex items-center gap-2">
					<Loader2 className="w-4 h-4 animate-spin" />
					{children}
				</span>
			) : icon ? (
				<span className="flex items-center gap-2">
					{icon}
					{children}
				</span>
			) : (
				children
			)}
		</button>
	);
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
	icon: ReactNode;
}

export function IconButton({
	icon,
	className = "",
	...props
}: IconButtonProps) {
	return (
		<button
			className={`p-2 text-gray-400 hover:text-gray-600 cursor-pointer rounded-md hover:bg-gray-100 ${className}`}
			{...props}
		>
			{icon}
		</button>
	);
}
