"use client";

import { AlertTriangle, Loader2, Play, Tag } from "lucide-react";
import { type DongleInfo, useOnlineDevicesDongleCheck } from "../hooks/useExtendedTest";
import { Modal } from "@/app/components/modal";
import { Button } from "@/app/components/button";

interface StartTestModalProps {
	isOpen: boolean;
	onClose: () => void;
	onStart: () => void;
	isPending: boolean;
	isError: boolean;
	durationMinutes: number;
	onDurationChange: (minutes: number) => void;
	// Selection from landing page
	selectedLabels?: string[];
	selectedDeviceCount?: number;
	selectionMode?: "labels" | "devices";
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
	selectedLabels = [],
	selectedDeviceCount = 0,
	selectionMode = "labels",
}: StartTestModalProps) {
	const { data: dongleInfo, isLoading: dongleCheckLoading } = useOnlineDevicesDongleCheck();

	const hasDongleDevices = dongleInfo && dongleInfo.dongleDevices.length > 0;

	const footer = (
		<>
			<Button 
				onClick={onClose}
				variant="secondary"
			>
				Cancel
			</Button>
			<Button
				onClick={onStart}
				disabled={isPending || dongleCheckLoading}
				loading={isPending}
				variant={hasDongleDevices ? "warning" : "primary"}
				icon={isPending ? undefined : <Play className="w-4 h-4" />}
			>
				{isPending ? "Starting..." : (hasDongleDevices ? "Start Anyway" : "Start Test")}
			</Button>
		</>
	);

	return (
		<Modal
			open={isOpen}
			onClose={onClose}
			title="Start Network Analysis"
			width="w-[520px]"
			footer={footer}
		>
			<div className="space-y-4">
				{/* Info Banner */}
				<div className="bg-blue-50 border border-blue-200 rounded-lg p-4">
					<p className="text-sm text-blue-800">
						This will run an extended network speed test on all online devices matching
						your label filter to measure network performance under load.
					</p>
				</div>

				{/* Selected Devices Display */}
				<div>
					<label className="block text-sm font-medium text-gray-700 mb-2">
						<div className="flex items-center space-x-2">
							<Tag className="w-4 h-4" />
							<span>Target Devices</span>
						</div>
					</label>
					<div className="px-3 py-2 bg-gray-50 border border-gray-200 rounded-lg">
						{selectionMode === "labels" && selectedLabels.length > 0 ? (
							<div className="flex flex-wrap gap-2">
								{selectedLabels.map((label) => (
									<span
										key={label}
										className="inline-flex items-center px-2 py-1 rounded-full text-sm bg-blue-100 text-blue-800"
									>
										{label}
									</span>
								))}
							</div>
						) : selectionMode === "devices" && selectedDeviceCount > 0 ? (
							<p className="text-sm text-gray-900">
								{selectedDeviceCount} device{selectedDeviceCount !== 1 ? "s" : ""} selected
							</p>
						) : (
							<p className="text-sm text-gray-500">All online devices</p>
						)}
					</div>
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
							<Button
								key={mins}
								onClick={() => onDurationChange(mins)}
								variant={durationMinutes === mins ? "primary" : "secondary"}
								className="flex-1"
							>
								{mins} min
							</Button>
						))}
					</div>
					<p className="mt-1 text-xs text-gray-500">
						Longer tests provide more data points, but use more data and stress the connection longer
					</p>
				</div>

				{/* Error Message */}
				{isError && (
					<div className="bg-red-50 border border-red-200 rounded-lg p-3">
						<p className="text-sm text-red-800">
							Failed to start test. Make sure there are online devices available.
						</p>
					</div>
				)}
			</div>
		</Modal>
	);
}
