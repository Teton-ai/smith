"use client";

import { ChevronDown, ChevronUp, Cpu, Loader2, TrendingDown, TrendingUp, Wifi } from "lucide-react";
import { useMemo, useState } from "react";
import type { DeviceExtendedTestResult, ExtendedTestStatus } from "../hooks/useExtendedTest";

interface DeviceTableProps {
	data: ExtendedTestStatus;
	onSelectDevice: (device: DeviceExtendedTestResult) => void;
	selectedDeviceId: number | null;
}

interface DeviceWithStats extends DeviceExtendedTestResult {
	avgDownload: number;
	avgUpload: number | null;
	trendPercent: number;
	stdDev: number;
}

function calculateDeviceStats(result: DeviceExtendedTestResult): DeviceWithStats {
	if (!result.minute_stats || result.minute_stats.length === 0) {
		return {
			...result,
			avgDownload: 0,
			avgUpload: null,
			trendPercent: 0,
			stdDev: 0,
		};
	}

	const downloadSpeeds = result.minute_stats.map((s) => s.download.average_mbps);
	const avgDownload =
		downloadSpeeds.reduce((a, b) => a + b, 0) / downloadSpeeds.length;

	// Standard deviation
	const variance =
		downloadSpeeds.reduce((sum, val) => sum + Math.pow(val - avgDownload, 2), 0) /
		downloadSpeeds.length;
	const stdDev = Math.sqrt(variance);

	// Upload average
	const uploadSpeeds = result.minute_stats
		.filter((s) => s.upload)
		.map((s) => s.upload!.average_mbps);
	const avgUpload =
		uploadSpeeds.length > 0
			? uploadSpeeds.reduce((a, b) => a + b, 0) / uploadSpeeds.length
			: null;

	// Trend (first vs last minute)
	const sorted = [...result.minute_stats].sort((a, b) => a.minute - b.minute);
	const firstMinute = sorted[0].download.average_mbps;
	const lastMinute = sorted[sorted.length - 1].download.average_mbps;
	const trendPercent =
		firstMinute > 0 ? ((lastMinute - firstMinute) / firstMinute) * 100 : 0;

	return {
		...result,
		avgDownload,
		avgUpload,
		trendPercent,
		stdDev,
	};
}

// Speed tier thresholds (matching InsightsCards.tsx)
function getSpeedTier(avgMbps: number): {
	label: string;
	color: string;
	bgColor: string;
} {
	if (avgMbps >= 100) {
		return { label: "Fast", color: "text-green-800", bgColor: "bg-green-100" };
	}
	if (avgMbps >= 50) {
		return { label: "Moderate", color: "text-blue-800", bgColor: "bg-blue-100" };
	}
	return { label: "Slow", color: "text-red-800", bgColor: "bg-red-100" };
}

function getStatusBadge(device: DeviceWithStats): {
	label: string;
	color: string;
	bgColor: string;
} | null {
	if (device.status !== "completed") {
		return null;
	}

	// Check for degradation first (takes priority)
	if (device.trendPercent < -20) {
		return { label: "Degrading", color: "text-orange-800", bgColor: "bg-orange-100" };
	}

	// High variance (stdDev > 30% of average)
	const cvPercent = device.avgDownload > 0 ? (device.stdDev / device.avgDownload) * 100 : 0;
	if (cvPercent > 30) {
		return { label: "Variable", color: "text-yellow-800", bgColor: "bg-yellow-100" };
	}

	// Use absolute speed tiers: Fast >= 100, Moderate >= 50, Slow < 50
	return getSpeedTier(device.avgDownload);
}

function getNetworkTypeIcon(device: DeviceExtendedTestResult) {
	if (!device.network_info) return null;

	switch (device.network_info.interface_type) {
		case "Wifi":
			return <Wifi className="w-4 h-4 text-blue-500" />;
		case "Ethernet":
			return <Cpu className="w-4 h-4 text-green-500" />;
		case "Lte":
			return <Wifi className="w-4 h-4 text-purple-500" />;
		default:
			return null;
	}
}

type SortField = "avgDownload" | "avgUpload" | "trend" | "serial";
type SortDirection = "asc" | "desc";

export default function DeviceTable({
	data,
	onSelectDevice,
	selectedDeviceId,
}: DeviceTableProps) {
	const [sortField, setSortField] = useState<SortField>("avgDownload");
	const [sortDirection, setSortDirection] = useState<SortDirection>("asc");

	const devicesWithStats = useMemo(
		() => data.results.map(calculateDeviceStats),
		[data.results]
	);

	const sortedDevices = useMemo(() => {
		return [...devicesWithStats].sort((a, b) => {
			let comparison = 0;

			switch (sortField) {
				case "avgDownload":
					comparison = a.avgDownload - b.avgDownload;
					break;
				case "avgUpload":
					comparison = (a.avgUpload || 0) - (b.avgUpload || 0);
					break;
				case "trend":
					comparison = a.trendPercent - b.trendPercent;
					break;
				case "serial":
					comparison = a.serial_number.localeCompare(b.serial_number);
					break;
			}

			return sortDirection === "asc" ? comparison : -comparison;
		});
	}, [devicesWithStats, sortField, sortDirection]);

	const handleSort = (field: SortField) => {
		if (sortField === field) {
			setSortDirection((prev) => (prev === "asc" ? "desc" : "asc"));
		} else {
			setSortField(field);
			setSortDirection(field === "serial" ? "asc" : "asc");
		}
	};

	const SortIcon = ({ field }: { field: SortField }) => {
		if (sortField !== field) {
			return <ChevronDown className="w-4 h-4 text-gray-300" />;
		}
		return sortDirection === "asc" ? (
			<ChevronUp className="w-4 h-4 text-indigo-600" />
		) : (
			<ChevronDown className="w-4 h-4 text-indigo-600" />
		);
	};

	return (
		<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
			<div className="px-4 py-3 border-b border-gray-200 bg-gray-50">
				<h3 className="text-sm font-semibold text-gray-900">
					Device Performance (Worst Performers First)
				</h3>
			</div>

			<div className="overflow-x-auto">
				<table className="min-w-full divide-y divide-gray-200">
					<thead className="bg-gray-50">
						<tr>
							<th
								scope="col"
								className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider cursor-pointer hover:bg-gray-100"
								onClick={() => handleSort("serial")}
							>
								<div className="flex items-center space-x-1">
									<span>Device</span>
									<SortIcon field="serial" />
								</div>
							</th>
							<th scope="col" className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
								Network
							</th>
							<th
								scope="col"
								className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider cursor-pointer hover:bg-gray-100"
								onClick={() => handleSort("avgDownload")}
							>
								<div className="flex items-center space-x-1">
									<span>Avg Download</span>
									<SortIcon field="avgDownload" />
								</div>
							</th>
							<th
								scope="col"
								className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider cursor-pointer hover:bg-gray-100"
								onClick={() => handleSort("avgUpload")}
							>
								<div className="flex items-center space-x-1">
									<span>Avg Upload</span>
									<SortIcon field="avgUpload" />
								</div>
							</th>
							<th
								scope="col"
								className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider cursor-pointer hover:bg-gray-100"
								onClick={() => handleSort("trend")}
							>
								<div className="flex items-center space-x-1">
									<span>Trend</span>
									<SortIcon field="trend" />
								</div>
							</th>
							<th scope="col" className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
								Status
							</th>
						</tr>
					</thead>
					<tbody className="bg-white divide-y divide-gray-200">
						{sortedDevices.map((device) => {
							const badge = getStatusBadge(device);
							const isSelected = selectedDeviceId === device.device_id;

							return (
								<tr
									key={device.device_id}
									onClick={() => onSelectDevice(device)}
									className={`cursor-pointer transition-colors ${
										isSelected
											? "bg-indigo-50 hover:bg-indigo-100"
											: "hover:bg-gray-50"
									}`}
								>
									<td className="px-4 py-3 whitespace-nowrap">
										<div className="flex items-center space-x-2">
											<Cpu className="w-4 h-4 text-gray-400" />
											<span className="text-sm font-medium text-gray-900">
												{device.serial_number}
											</span>
										</div>
									</td>
									<td className="px-4 py-3 whitespace-nowrap">
										<div className="flex items-center space-x-1">
											{getNetworkTypeIcon(device)}
											<span className="text-sm text-gray-600">
												{device.network_info?.interface_type || "-"}
											</span>
										</div>
									</td>
									<td className="px-4 py-3 whitespace-nowrap">
										{device.status === "completed" ? (
											<span className="text-sm font-mono text-gray-900">
												{device.avgDownload.toFixed(1)} Mbps
											</span>
										) : device.status === "running" ? (
											<div className="flex items-center space-x-1 text-blue-600">
												<Loader2 className="w-4 h-4 animate-spin" />
												<span className="text-sm">Running</span>
											</div>
										) : (
											<span className="text-sm text-gray-400">
												{device.status}
											</span>
										)}
									</td>
									<td className="px-4 py-3 whitespace-nowrap">
										<span className="text-sm font-mono text-gray-900">
											{device.avgUpload !== null
												? `${device.avgUpload.toFixed(1)} Mbps`
												: "-"}
										</span>
									</td>
									<td className="px-4 py-3 whitespace-nowrap">
										{device.status === "completed" ? (
											<div className="flex items-center space-x-1">
												{device.trendPercent < 0 ? (
													<TrendingDown className="w-4 h-4 text-red-500" />
												) : (
													<TrendingUp className="w-4 h-4 text-green-500" />
												)}
												<span
													className={`text-sm font-mono ${
														device.trendPercent < -10
															? "text-red-600"
															: device.trendPercent < 0
																? "text-yellow-600"
																: "text-green-600"
													}`}
												>
													{device.trendPercent >= 0 ? "+" : ""}
													{device.trendPercent.toFixed(1)}%
												</span>
											</div>
										) : (
											<span className="text-sm text-gray-400">-</span>
										)}
									</td>
									<td className="px-4 py-3 whitespace-nowrap">
										{badge ? (
											<span
												className={`px-2 py-1 text-xs font-medium rounded-full ${badge.bgColor} ${badge.color}`}
											>
												{badge.label}
											</span>
										) : (
											<span className="px-2 py-1 text-xs font-medium rounded-full bg-gray-100 text-gray-600">
												{device.status}
											</span>
										)}
									</td>
								</tr>
							);
						})}
					</tbody>
				</table>
			</div>
		</div>
	);
}
