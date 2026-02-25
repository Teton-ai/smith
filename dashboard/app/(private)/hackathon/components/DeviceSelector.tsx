"use client";

import { Cpu, Loader2, Search, Tag, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { type Device, useGetDevices } from "@/app/api-client";
import LabelAutocomplete from "@/app/components/LabelAutocomplete";

export type SelectionMode = "labels" | "devices";

interface DeviceSelectorProps {
	mode: SelectionMode;
	onModeChange: (mode: SelectionMode) => void;
	selectedLabels: string[];
	onLabelsChange: (labels: string[]) => void;
	selectedDeviceIds: Set<number>;
	onDeviceIdsChange: (ids: Set<number>) => void;
	onDevicesResolved: (devices: Device[]) => void;
}

export default function DeviceSelector({
	mode,
	onModeChange,
	selectedLabels,
	onLabelsChange,
	selectedDeviceIds,
	onDeviceIdsChange,
	onDevicesResolved,
}: DeviceSelectorProps) {
	const [deviceSearchQuery, setDeviceSearchQuery] = useState("");

	// Fetch online devices for manual selection
	const { data: onlineDevices = [], isLoading: devicesLoading } = useGetDevices(
		{ online: true },
		{ query: { enabled: mode === "devices" } }
	);

	// Fetch devices matching labels (for labels mode preview)
	const { data: labelMatchDevices = [], isLoading: labelDevicesLoading } =
		useGetDevices(
			{ online: true, labels: selectedLabels },
			{ query: { enabled: mode === "labels" && selectedLabels.length > 0 } }
		);

	// Filter devices for search in devices mode
	const filteredDevices = useMemo(() => {
		if (!deviceSearchQuery) return onlineDevices;
		const q = deviceSearchQuery.toLowerCase();
		return onlineDevices.filter((d: Device) =>
			d.serial_number.toLowerCase().includes(q)
		);
	}, [onlineDevices, deviceSearchQuery]);

	// Get the resolved devices based on mode
	const resolvedDevices = useMemo(() => {
		if (mode === "labels") {
			return labelMatchDevices;
		}
		return onlineDevices.filter((d) => selectedDeviceIds.has(d.id));
	}, [mode, labelMatchDevices, onlineDevices, selectedDeviceIds]);

	// Track previous device IDs to avoid unnecessary updates
	const prevDeviceIdsRef = useRef<string>("");

	// Notify parent of resolved devices only when the set actually changes
	useEffect(() => {
		const currentIds = resolvedDevices.map(d => d.id).sort().join(",");
		if (currentIds !== prevDeviceIdsRef.current) {
			prevDeviceIdsRef.current = currentIds;
			onDevicesResolved(resolvedDevices);
		}
	}, [resolvedDevices, onDevicesResolved]);

	const handleToggleDevice = (deviceId: number) => {
		const newIds = new Set(selectedDeviceIds);
		if (newIds.has(deviceId)) {
			newIds.delete(deviceId);
		} else {
			newIds.add(deviceId);
		}
		onDeviceIdsChange(newIds);
	};

	const handleAddLabel = (label: string) => {
		if (!selectedLabels.includes(label)) {
			onLabelsChange([...selectedLabels, label]);
		}
	};

	const handleRemoveLabel = (label: string) => {
		onLabelsChange(selectedLabels.filter((l) => l !== label));
	};

	return (
		<div className="space-y-4">
			{/* Mode Selector */}
			<div className="flex space-x-2">
				<button
					onClick={() => onModeChange("labels")}
					className={`flex items-center space-x-2 px-4 py-2 rounded-lg border text-sm font-medium transition-colors ${
						mode === "labels"
							? "bg-indigo-600 text-white border-indigo-600"
							: "bg-white text-gray-700 border-gray-200 hover:bg-gray-50"
					}`}
				>
					<Tag className="w-4 h-4" />
					<span>By Labels</span>
				</button>
				<button
					onClick={() => onModeChange("devices")}
					className={`flex items-center space-x-2 px-4 py-2 rounded-lg border text-sm font-medium transition-colors ${
						mode === "devices"
							? "bg-indigo-600 text-white border-indigo-600"
							: "bg-white text-gray-700 border-gray-200 hover:bg-gray-50"
					}`}
				>
					<Cpu className="w-4 h-4" />
					<span>By Devices</span>
				</button>
			</div>

			{/* Labels Mode */}
			{mode === "labels" && (
				<div className="space-y-3">
					<LabelAutocomplete
						onSelect={handleAddLabel}
						existingFilters={selectedLabels}
					/>
					{selectedLabels.length > 0 && (
						<div className="flex flex-wrap gap-2">
							{selectedLabels.map((label) => (
								<span
									key={label}
									className="inline-flex items-center px-2.5 py-1 rounded-full text-sm bg-indigo-100 text-indigo-800"
								>
									{label}
									<button
										onClick={() => handleRemoveLabel(label)}
										className="ml-1.5 hover:text-indigo-600"
									>
										<X className="w-3.5 h-3.5" />
									</button>
								</span>
							))}
						</div>
					)}
					{labelDevicesLoading ? (
						<div className="flex items-center space-x-2 text-gray-500 text-sm">
							<Loader2 className="w-4 h-4 animate-spin" />
							<span>Finding matching devices...</span>
						</div>
					) : selectedLabels.length > 0 ? (
						<p className="text-sm text-gray-600">
							{labelMatchDevices.length} online device
							{labelMatchDevices.length !== 1 ? "s" : ""} match
							{labelMatchDevices.length === 1 ? "es" : ""} selected labels
						</p>
					) : (
						<p className="text-sm text-gray-500">
							Select labels to filter devices
						</p>
					)}
				</div>
			)}

			{/* Devices Mode */}
			{mode === "devices" && (
				<div className="space-y-3">
					<div className="relative">
						<Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
						<input
							type="text"
							placeholder="Search by serial number..."
							value={deviceSearchQuery}
							onChange={(e) => setDeviceSearchQuery(e.target.value)}
							className="w-full pl-10 pr-4 py-2 text-sm text-gray-900 placeholder-gray-400 bg-white border border-gray-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
						/>
					</div>

					{devicesLoading ? (
						<div className="flex items-center justify-center py-8">
							<Loader2 className="w-6 h-6 animate-spin text-indigo-600" />
						</div>
					) : (
						<div className="max-h-64 overflow-y-auto border border-gray-200 rounded-lg">
							{filteredDevices.length === 0 ? (
								<div className="p-4 text-center text-gray-500 text-sm">
									No online devices found
								</div>
							) : (
								filteredDevices.map((device: Device) => {
									const isSelected = selectedDeviceIds.has(device.id);
									return (
										<button
											key={device.id}
											onClick={() => handleToggleDevice(device.id)}
											className={`w-full px-4 py-2 text-left text-sm border-b border-gray-100 last:border-b-0 transition-colors ${
												isSelected
													? "bg-indigo-100 text-indigo-900"
													: "bg-white text-gray-900 hover:bg-gray-50"
											}`}
										>
											<div className="flex items-center justify-between">
												<div className="flex items-center space-x-3">
													<input
														type="checkbox"
														checked={isSelected}
														readOnly
														className="w-4 h-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
													/>
													<span className="font-mono">
														{device.serial_number}
													</span>
												</div>
												{device.labels && Object.keys(device.labels).length > 0 && (
													<div className="flex gap-1">
														{Object.entries(device.labels).slice(0, 2).map(([key, value]) => (
															<span
																key={`${key}=${value}`}
																className="px-1.5 py-0.5 text-xs bg-gray-100 text-gray-600 rounded"
															>
																{key}={value}
															</span>
														))}
														{Object.keys(device.labels).length > 2 && (
															<span className="text-xs text-gray-400">
																+{Object.keys(device.labels).length - 2}
															</span>
														)}
													</div>
												)}
											</div>
										</button>
									);
								})
							)}
						</div>
					)}

					<p className="text-sm text-gray-600">
						{selectedDeviceIds.size} device
						{selectedDeviceIds.size !== 1 ? "s" : ""} selected
					</p>
				</div>
			)}
		</div>
	);
}
