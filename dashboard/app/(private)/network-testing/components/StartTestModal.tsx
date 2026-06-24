"use client";

import { AlertTriangle, Loader2, Play } from "lucide-react";
import { useCallback, useState } from "react";
import type { Device } from "@/app/api-client";
import { Button } from "@/app/components/button";
import { Modal } from "@/app/components/modal";
import {
	type DongleInfo,
	useOnlineDevicesDongleCheck,
	useStartExtendedTest,
} from "../hooks/useExtendedTest";
import DeviceSelector, { type SelectionMode } from "./DeviceSelector";

interface StartTestModalProps {
	isOpen: boolean;
	onClose: () => void;
	onStarted: (sessionId: string) => void;
}

function estimateDataUsageMB(durationMinutes: number): string {
	// Rough estimate: continuous download at ~10 Mbps average
	// 10 Mbit/s * duration_seconds / 8 = MB
	const lowEstimate = Math.round((5 * durationMinutes * 60) / 8);
	const highEstimate = Math.round((20 * durationMinutes * 60) / 8);
	return `${lowEstimate}-${highEstimate} MB`;
}

function DongleWarning({
	dongleInfo,
	durationMinutes,
}: {
	dongleInfo: DongleInfo;
	durationMinutes: number;
}) {
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
						<strong>{dongleDevices.length}</strong> of {totalOnlineDevices}{" "}
						devices {dongleDevices.length === 1 ? "is" : "are"} connected via
						cellular (dongle/LTE).
					</p>
					<p className="text-sm text-amber-700 mt-1">
						A {durationMinutes}-minute test may use approximately{" "}
						<strong>{dataEstimate}</strong> of cellular data per device.
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
	onStarted,
}: StartTestModalProps) {
	const [selectionMode, setSelectionMode] = useState<SelectionMode>("labels");
	const [selectedLabels, setSelectedLabels] = useState<string[]>([]);
	const [selectedDeviceIds, setSelectedDeviceIds] = useState<Set<number>>(
		new Set(),
	);
	const [resolvedDevices, setResolvedDevices] = useState<Device[]>([]);
	const [durationMinutes, setDurationMinutes] = useState(3);

	const startTestMutation = useStartExtendedTest();
	const { data: dongleInfo, isLoading: dongleCheckLoading } =
		useOnlineDevicesDongleCheck();

	const handleDevicesResolved = useCallback((devices: Device[]) => {
		setResolvedDevices(devices);
	}, []);

	const hasSelection =
		selectionMode === "labels"
			? selectedLabels.length > 0
			: resolvedDevices.length > 0;

	const hasDongleDevices = dongleInfo && dongleInfo.dongleDevices.length > 0;

	const handleStart = async () => {
		const labelFilter =
			selectionMode === "labels" ? selectedLabels.join(",") : "";
		const serialNumbers =
			selectionMode === "devices"
				? resolvedDevices.map((d) => d.serial_number)
				: undefined;
		try {
			const result = await startTestMutation.mutateAsync({
				label_filter: labelFilter,
				serial_numbers: serialNumbers,
				duration_minutes: durationMinutes,
			});
			onStarted(result.session_id);
		} catch (error) {
			console.error("Failed to start test:", error);
		}
	};

	const footer = (
		<>
			<Button onClick={onClose} variant="secondary">
				Cancel
			</Button>
			<Button
				onClick={handleStart}
				disabled={
					startTestMutation.isPending || dongleCheckLoading || !hasSelection
				}
				loading={startTestMutation.isPending}
				variant={hasDongleDevices ? "warning" : "primary"}
				icon={
					startTestMutation.isPending ? undefined : <Play className="w-4 h-4" />
				}
			>
				{startTestMutation.isPending
					? "Starting..."
					: hasDongleDevices
						? "Start Anyway"
						: "Start Test"}
			</Button>
		</>
	);

	return (
		<Modal
			open={isOpen}
			onClose={onClose}
			title="Start Network Test"
			width="w-[560px]"
			footer={footer}
		>
			<div className="space-y-4">
				{/* Info Banner */}
				<div className="bg-blue-50 border border-blue-200 rounded-lg p-4">
					<p className="text-sm text-blue-800">
						This runs an extended network speed test on the selected online
						devices to measure network performance under load.
					</p>
				</div>

				{/* Device Selection */}
				<DeviceSelector
					mode={selectionMode}
					onModeChange={setSelectionMode}
					selectedLabels={selectedLabels}
					onLabelsChange={setSelectedLabels}
					selectedDeviceIds={selectedDeviceIds}
					onDeviceIdsChange={setSelectedDeviceIds}
					onDevicesResolved={handleDevicesResolved}
				/>

				{/* Dongle Warning */}
				{dongleCheckLoading ? (
					<div className="flex items-center space-x-2 text-gray-500 text-sm">
						<Loader2 className="w-4 h-4 animate-spin" />
						<span>Checking device connections...</span>
					</div>
				) : dongleInfo ? (
					<DongleWarning
						dongleInfo={dongleInfo}
						durationMinutes={durationMinutes}
					/>
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
								onClick={() => setDurationMinutes(mins)}
								variant={durationMinutes === mins ? "primary" : "secondary"}
								className="flex-1"
							>
								{mins} min
							</Button>
						))}
					</div>
					<p className="mt-1 text-xs text-gray-500">
						Longer tests provide more data points, but use more data and stress
						the connection longer
					</p>
				</div>

				{/* Error Message */}
				{startTestMutation.isError && (
					<div className="bg-red-50 border border-red-200 rounded-lg p-3">
						<p className="text-sm text-red-800">
							Failed to start test. Make sure there are online devices
							available.
						</p>
					</div>
				)}
			</div>
		</Modal>
	);
}
