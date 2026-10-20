"use client";

import { Button } from "@teton/smith-ui";
import { AlertTriangle, X } from "lucide-react";
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

/**
 * Confirmation before skipping a staggered rollout. Shared by Finish now and
 * the Deploy to Selected opt-out so both warn the same way.
 */
export function EmergencyConfirmModal({
	open,
	title,
	confirmLabel,
	loading = false,
	onCancel,
	onConfirm,
}: {
	open: boolean;
	title: string;
	confirmLabel: string;
	loading?: boolean;
	onCancel: () => void;
	onConfirm: () => void;
}) {
	return (
		<Modal
			open={open}
			onClose={onCancel}
			title={title}
			footer={
				<>
					<Button
						variant="soft"
						tone="gray"
						disabled={loading}
						onClick={onCancel}
					>
						Cancel
					</Button>
					<Button
						variant="solid"
						tone="red"
						loading={loading}
						onClick={onConfirm}
					>
						{confirmLabel}
					</Button>
				</>
			}
		>
			<div className="flex gap-3">
				<AlertTriangle className="w-5 h-5 text-red-600 flex-shrink-0 mt-0.5" />
				<div className="space-y-2 text-sm text-gray-700">
					<p>
						Some departments may have trouble updating all their devices at
						once: when every device on a site downloads the release together,
						the site's network can saturate and devices can go offline.
					</p>
					<p className="font-medium text-gray-900">
						Only do this in case of emergency.
					</p>
				</div>
			</div>
		</Modal>
	);
}
