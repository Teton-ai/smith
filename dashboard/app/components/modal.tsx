"use client";

import { X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

interface ModalProps {
	open: boolean;
	onClose: () => void;
	title: string;
	subtitle?: string;
	width?: string;
	children: ReactNode;
	footer?: ReactNode;
	headerRight?: ReactNode;
}

export function Modal({
	open,
	onClose,
	title,
	subtitle,
	width = "w-[520px]",
	children,
	footer,
	headerRight,
}: ModalProps) {
	const [mounted, setMounted] = useState(false);
	const [visible, setVisible] = useState(false);
	const backdropRef = useRef<HTMLDivElement>(null);

	useEffect(() => {
		setMounted(true);
	}, []);

	useEffect(() => {
		if (open) {
			requestAnimationFrame(() => setVisible(true));
		} else {
			setVisible(false);
		}
	}, [open]);

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
			ref={backdropRef}
			onClick={(e) => {
				if (e.target === backdropRef.current) onClose();
			}}
			className={`fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4 transition-opacity duration-200 ${
				visible ? "opacity-100" : "opacity-0"
			}`}
		>
			<div
				className={`bg-white rounded-lg shadow-xl max-h-[90vh] flex flex-col ${width} max-w-full transition-all duration-200 ${
					visible ? "opacity-100 scale-100" : "opacity-0 scale-95"
				}`}
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
