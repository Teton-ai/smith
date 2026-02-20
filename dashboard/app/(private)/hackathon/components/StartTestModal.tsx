"use client";

import { AlertTriangle, Loader2, Play, Tag, X } from "lucide-react";
import { type DongleInfo, useOnlineDevicesDongleCheck } from "../hooks/useExtendedTest";

interface StartTestModalProps {
	isOpen: boolean;
	onClose: () => void;
	onStart: () => void;
	isPending: boolean;
	isError: boolean;
	durationMinutes: number;
	onDurationChange: (minutes: number) => void;
	labelFilter: string;
	onLabelFilterChange: (filter: string) => void;
}

function estimateDataUsageMB(durationMinutes: number): string {
	// Rough estimate: continuous download at ~10 Mbps average
	// 10 Mbit/s * duration_seconds / 8 = MB
	const lowEstimate = Math.round((5 * durationMinutes * 60) / 8);
	const highEstimate = Math.round((20 * durationMinutes * 60) / 8);
	return `${lowEstimate}-${highEstimate} MB`;
}

function DongleWarning({ dongleInfo, durationMinutes }: { dongleInfo: DongleInfo; durationMinutes: number }) {
	const { dongleDevices, totalOnlineDevices } = dongleInfo;

	if (dongleDevices.length === 0) return null;

	const dataEstimate = estimateDataUsageMB(durationMinutes);

	return (
		<div className="bg-amber-50 border border-amber-200 rounded-lg p-4">
			<div className="flex items-start space-x-3">
				<AlertTriangle className="w-5 h-5 text-amber-600 flex-shrink-0 mt-0.5" />
				<div className="flex-1">
					<p className="text-sm font-medium text-amber-800">
						Cellular Data Warning
					</p>
					<p className="text-sm text-amber-700 mt-1">
						<strong>{dongleDevices.length}</strong> of {totalOnlineDevices} devices{" "}
						{dongleDevices.length === 1 ? "is" : "are"} connected via cellular (dongle/LTE).
					</p>
					<p className="text-sm text-amber-700 mt-1">
						A {durationMinutes}-minute test may use approximately <strong>{dataEstimate}</strong> of
						cellular data per device.
					</p>
					{dongleDevices.length <= 5 && (
						<p className="text-xs text-amber-600 mt-2 font-mono">
							{dongleDevices.map((d) => d.serial_number).join(", ")}
						</p>
					)}
				</div>
			</div>
		</div>
	);
}

export default function StartTestModal({
	isOpen,
	onClose,
	onStart,
	isPending,
	isError,
	durationMinutes,
	onDurationChange,
	labelFilter,
	onLabelFilterChange,
}: StartTestModalProps) {
	const { data: dongleInfo, isLoading: dongleCheckLoading } = useOnlineDevicesDongleCheck();

	if (!isOpen) return null;

	const hasDongleDevices = dongleInfo && dongleInfo.dongleDevices.length > 0;

	return (
		<div className="fixed inset-0 z-50 flex items-center justify-center">
			{/* Backdrop */}
			<div
				className="absolute inset-0 bg-black/50"
				onClick={onClose}
				onKeyDown={(e) => e.key === "Escape" && onClose()}
				role="button"
				tabIndex={0}
				aria-label="Close modal"
			/>

			{/* Modal */}
			<div className="relative bg-white rounded-lg shadow-xl w-full max-w-md mx-4">
				<div className="flex items-center justify-between px-6 py-4 border-b border-gray-200">
					<h3 className="text-lg font-semibold text-gray-900">
						Start Network Analysis
					</h3>
					<button
						onClick={onClose}
						className="p-1 rounded-md hover:bg-gray-100 transition-colors"
					>
						<X className="w-5 h-5 text-gray-400" />
					</button>
				</div>

				<div className="px-6 py-4 space-y-4">
					{/* Info Banner */}
					<div className="bg-indigo-50 border border-indigo-200 rounded-lg p-4">
						<p className="text-sm text-indigo-800">
							This will run an extended network speed test on all online devices matching
							your label filter to measure network performance under load.
						</p>
					</div>

					{/* Label Filter Input */}
					<div>
						<label className="block text-sm font-medium text-gray-700 mb-2">
							<div className="flex items-center space-x-2">
								<Tag className="w-4 h-4" />
								<span>Label Filter</span>
							</div>
						</label>
						<input
							type="text"
							value={labelFilter}
							onChange={(e) => onLabelFilterChange(e.target.value)}
							placeholder="e.g., location=office or env=prod"
							className="w-full px-3 py-2 border border-gray-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
						/>
						<p className="mt-1 text-xs text-gray-500">
							Format: key=value (leave empty for all online devices)
						</p>
					</div>

					{/* Dongle Warning */}
					{dongleCheckLoading ? (
						<div className="flex items-center space-x-2 text-gray-500 text-sm">
							<Loader2 className="w-4 h-4 animate-spin" />
							<span>Checking device connections...</span>
						</div>
					) : dongleInfo ? (
						<DongleWarning dongleInfo={dongleInfo} durationMinutes={durationMinutes} />
					) : null}

					{/* Duration Selector */}
					<div>
						<label className="block text-sm font-medium text-gray-700 mb-2">
							Test Duration
						</label>
						<div className="flex space-x-2">
							{[3, 5, 8].map((mins) => (
								<button
									key={mins}
									onClick={() => onDurationChange(mins)}
									className={`flex-1 py-2 px-4 rounded-lg border text-sm font-medium transition-colors ${
										durationMinutes === mins
											? "bg-indigo-600 text-white border-indigo-600"
											: "bg-white text-gray-700 border-gray-200 hover:bg-gray-50"
									}`}
								>
									{mins} min
								</button>
							))}
						</div>
						<p className="mt-1 text-xs text-gray-500">
							Longer tests provide more data points but keep devices busy longer
						</p>
					</div>
				</div>

				<div className="flex items-center justify-end space-x-3 px-6 py-4 border-t border-gray-200 bg-gray-50 rounded-b-lg">
					<button
						onClick={onClose}
						className="px-4 py-2 text-sm font-medium text-gray-700 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors"
					>
						Cancel
					</button>
					<button
						onClick={onStart}
						disabled={isPending || dongleCheckLoading}
						className={`flex items-center space-x-2 px-4 py-2 text-sm font-medium text-white rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
							hasDongleDevices
								? "bg-amber-600 hover:bg-amber-700"
								: "bg-indigo-600 hover:bg-indigo-700"
						}`}
					>
						{isPending ? (
							<>
								<Loader2 className="w-4 h-4 animate-spin" />
								<span>Starting...</span>
							</>
						) : (
							<>
								<Play className="w-4 h-4" />
								<span>{hasDongleDevices ? "Start Anyway" : "Start Test"}</span>
							</>
						)}
					</button>
				</div>

				{/* Error Message */}
				{isError && (
					<div className="px-6 pb-4">
						<div className="bg-red-50 border border-red-200 rounded-lg p-3">
							<p className="text-sm text-red-800">
								Failed to start test. Make sure there are online devices available.
							</p>
						</div>
					</div>
				)}
			</div>
		</div>
	);
}
