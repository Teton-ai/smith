"use client";

import { X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { createPortal } from "react-dom";

interface SidePanelProps {
	open: boolean;
	onClose: () => void;
	title: string;
	subtitle?: string;
	width?: string;
	children: ReactNode;
	footer?: ReactNode;
	headerRight?: ReactNode;
}

export function SidePanel({
	open,
	onClose,
	title,
	subtitle,
	width = "w-[420px]",
	children,
	footer,
	headerRight,
}: SidePanelProps) {
	const [mounted, setMounted] = useState(false);

	useEffect(() => {
		setMounted(true);
	}, []);

	useEffect(() => {
		if (!open) return;
		const handleKeyDown = (e: KeyboardEvent) => {
			if (e.key === "Escape") onClose();
		};
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [open, onClose]);

	if (!mounted || !open) return null;

	return createPortal(
		<div
			onClick={(e) => {
				if (e.target === e.currentTarget) onClose();
			}}
			className="fixed inset-0 bg-black/30 z-50 animate-fade-in"
		>
			<div
				className={`absolute inset-y-0 right-0 ${width} max-w-full bg-white shadow-xl flex flex-col animate-slide-in-right`}
			>
				{/* Header */}
				<div className="flex items-center justify-between p-6 pb-4">
					<div>
						<h3 className="text-lg font-semibold text-gray-900">{title}</h3>
						{subtitle && (
							<p className="text-xs text-gray-500 mt-0.5">{subtitle}</p>
						)}
					</div>
					<div className="flex items-center gap-3">
						{headerRight}
						<button
							onClick={onClose}
							className="text-gray-400 hover:text-gray-600 cursor-pointer"
						>
							<X className="w-5 h-5" />
						</button>
					</div>
				</div>
				<hr className="border-gray-200" />

				{/* Body */}
				<div className="p-6 overflow-y-auto flex-1">{children}</div>

				{/* Footer */}
				{footer && (
					<>
						<hr className="border-gray-200" />
						<div className="flex justify-end space-x-3 p-6 pt-4">{footer}</div>
					</>
				)}
			</div>
		</div>,
		document.body,
	);
}
