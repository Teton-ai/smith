"use client";

import { AlertTriangle, Cpu, Signal, Wifi, X } from "lucide-react";
import {
	CartesianGrid,
	Legend,
	Line,
	LineChart,
	ResponsiveContainer,
	Tooltip,
	XAxis,
	YAxis,
} from "recharts";
import type { DeviceExtendedTestResult, NetworkDetails, WifiDetails } from "../hooks/useExtendedTest";

interface DeviceInspectorProps {
	device: DeviceExtendedTestResult;
	onClose: () => void;
}

function getWifiDetails(details: NetworkDetails): WifiDetails | null {
	if ("Wifi" in details) {
		return details.Wifi;
	}
	return null;
}

function generateDiagnosis(device: DeviceExtendedTestResult): string[] {
	const diagnoses: string[] = [];

	if (!device.minute_stats || device.minute_stats.length === 0) {
		return ["No data available for analysis"];
	}

	const downloadSpeeds = device.minute_stats.map((s) => s.download.average_mbps);
	const avgDownload =
		downloadSpeeds.reduce((a, b) => a + b, 0) / downloadSpeeds.length;

	// Speed drop analysis
	const sorted = [...device.minute_stats].sort((a, b) => a.minute - b.minute);
	const firstMinute = sorted[0].download.average_mbps;
	const lastMinute = sorted[sorted.length - 1].download.average_mbps;
	const trendPercent =
		firstMinute > 0 ? ((lastMinute - firstMinute) / firstMinute) * 100 : 0;

	if (trendPercent < -30) {
		diagnoses.push(
			`Speed dropped ${Math.abs(trendPercent).toFixed(0)}% over test duration - possible thermal throttling or network congestion`
		);
	} else if (trendPercent < -20) {
		diagnoses.push(
			`Speed decreased ${Math.abs(trendPercent).toFixed(0)}% during test - may indicate bandwidth contention`
		);
	}

	// Variance analysis
	const variance =
		downloadSpeeds.reduce((sum, val) => sum + Math.pow(val - avgDownload, 2), 0) /
		downloadSpeeds.length;
	const stdDev = Math.sqrt(variance);
	const cv = (stdDev / avgDownload) * 100;

	if (cv > 40) {
		diagnoses.push(
			"High variance suggests intermittent connection or wireless interference"
		);
	} else if (cv > 25) {
		diagnoses.push("Moderate speed fluctuations detected");
	}

	// Upload vs Download analysis
	const uploadSpeeds = device.minute_stats
		.filter((s) => s.upload)
		.map((s) => s.upload!.average_mbps);
	if (uploadSpeeds.length > 0) {
		const avgUpload =
			uploadSpeeds.reduce((a, b) => a + b, 0) / uploadSpeeds.length;
		if (avgDownload > avgUpload * 10) {
			diagnoses.push(
				"Upload significantly lower than download - typical for asymmetric connections"
			);
		}
	}

	// WiFi signal analysis
	if (device.network_info) {
		const wifi = getWifiDetails(device.network_info.details);
		if (wifi) {
			if (wifi.signal_dbm < -75) {
				diagnoses.push(
					`Weak WiFi signal (${wifi.signal_dbm} dBm) - consider moving device closer to access point`
				);
			} else if (wifi.signal_dbm < -65) {
				diagnoses.push(
					`Fair WiFi signal (${wifi.signal_dbm} dBm) - signal could be improved`
				);
			}
		}
	}

	// Speed tier
	if (avgDownload < 25) {
		diagnoses.push("Slow connection speed may impact device operations");
	}

	if (diagnoses.length === 0) {
		diagnoses.push("Connection appears healthy with consistent performance");
	}

	return diagnoses;
}

export default function DeviceInspector({ device, onClose }: DeviceInspectorProps) {
	const chartData =
		device.minute_stats
			?.sort((a, b) => a.minute - b.minute)
			.map((stat) => ({
				minute: `Min ${stat.minute}`,
				download: stat.download.average_mbps,
				upload: stat.upload?.average_mbps ?? null,
				downloadStdDev: stat.download.std_dev,
			})) || [];

	const diagnoses = generateDiagnosis(device);
	const wifiDetails = device.network_info
		? getWifiDetails(device.network_info.details)
		: null;

	return (
		<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
			<div className="px-4 py-3 border-b border-gray-200 bg-gray-50 flex items-center justify-between">
				<div className="flex items-center space-x-3">
					<Cpu className="w-5 h-5 text-gray-500" />
					<div>
						<h3 className="text-sm font-semibold text-gray-900">
							{device.serial_number}
						</h3>
						<p className="text-xs text-gray-500">Device Inspector</p>
					</div>
				</div>
				<button
					onClick={onClose}
					className="p-1 rounded-md hover:bg-gray-100 transition-colors"
				>
					<X className="w-5 h-5 text-gray-400" />
				</button>
			</div>

			<div className="p-4 space-y-6">
				{/* Speed Chart */}
				<div>
					<h4 className="text-sm font-medium text-gray-700 mb-3">
						Speed Over Time
					</h4>
					{chartData.length > 0 ? (
						<div className="h-64">
							<ResponsiveContainer width="100%" height="100%">
								<LineChart
									data={chartData}
									margin={{ top: 5, right: 30, left: 20, bottom: 5 }}
								>
									<CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
									<XAxis
										dataKey="minute"
										tick={{ fontSize: 12, fill: "#6b7280" }}
									/>
									<YAxis
										tick={{ fontSize: 12, fill: "#6b7280" }}
										label={{
											value: "Mbps",
											angle: -90,
											position: "insideLeft",
											style: { fontSize: 12, fill: "#6b7280" },
										}}
									/>
									<Tooltip
										contentStyle={{
											backgroundColor: "white",
											border: "1px solid #e5e7eb",
											borderRadius: "6px",
											fontSize: "12px",
										}}
										formatter={(value: number | undefined) => value !== undefined ? `${value.toFixed(2)} Mbps` : ""}
									/>
									<Legend wrapperStyle={{ fontSize: "12px" }} />
									<Line
										type="monotone"
										dataKey="download"
										stroke="#6366f1"
										strokeWidth={2}
										dot={{ fill: "#6366f1", strokeWidth: 2, r: 4 }}
										name="Download"
									/>
									{chartData.some((d) => d.upload !== null) && (
										<Line
											type="monotone"
											dataKey="upload"
											stroke="#10b981"
											strokeWidth={2}
											dot={{ fill: "#10b981", strokeWidth: 2, r: 4 }}
											name="Upload"
										/>
									)}
								</LineChart>
							</ResponsiveContainer>
						</div>
					) : (
						<div className="h-64 flex items-center justify-center bg-gray-50 rounded-lg">
							<p className="text-gray-500">No data available</p>
						</div>
					)}
				</div>

				{/* Network Info */}
				{device.network_info && (
					<div>
						<h4 className="text-sm font-medium text-gray-700 mb-3">
							Network Information
						</h4>
						<div className="bg-gray-50 rounded-lg p-4 space-y-3">
							<div className="flex items-center justify-between">
								<div className="flex items-center space-x-2">
									{device.network_info.interface_type === "Wifi" ? (
										<Wifi className="w-4 h-4 text-blue-500" />
									) : (
										<Cpu className="w-4 h-4 text-green-500" />
									)}
									<span className="text-sm font-medium text-gray-700">
										{device.network_info.interface_type}
									</span>
								</div>
								<span className="text-sm text-gray-500">
									{device.network_info.interface_name}
								</span>
							</div>

							{wifiDetails && (
								<div className="grid grid-cols-2 gap-3 pt-2 border-t border-gray-200">
									<div>
										<span className="text-xs text-gray-500">SSID</span>
										<p className="text-sm font-medium text-gray-900">
											{wifiDetails.ssid}
										</p>
									</div>
									<div>
										<span className="text-xs text-gray-500">Signal Strength</span>
										<div className="flex items-center space-x-1">
											<Signal
												className={`w-4 h-4 ${
													wifiDetails.signal_dbm > -65
														? "text-green-500"
														: wifiDetails.signal_dbm > -75
															? "text-yellow-500"
															: "text-red-500"
												}`}
											/>
											<p className="text-sm font-medium text-gray-900">
												{wifiDetails.signal_dbm} dBm
											</p>
										</div>
									</div>
									<div>
										<span className="text-xs text-gray-500">Frequency</span>
										<p className="text-sm font-medium text-gray-900">
											{wifiDetails.frequency_mhz} MHz
										</p>
									</div>
									{wifiDetails.channel_width_mhz && (
										<div>
											<span className="text-xs text-gray-500">Channel Width</span>
											<p className="text-sm font-medium text-gray-900">
												{wifiDetails.channel_width_mhz} MHz
											</p>
										</div>
									)}
									{wifiDetails.vht_mcs !== null && (
										<div>
											<span className="text-xs text-gray-500">VHT-MCS</span>
											<p className="text-sm font-medium text-gray-900">
												{wifiDetails.vht_mcs}
											</p>
										</div>
									)}
									{wifiDetails.vht_nss !== null && (
										<div>
											<span className="text-xs text-gray-500">VHT-NSS</span>
											<p className="text-sm font-medium text-gray-900">
												{wifiDetails.vht_nss}
											</p>
										</div>
									)}
								</div>
							)}
						</div>
					</div>
				)}

				{/* Diagnosis */}
				<div>
					<h4 className="text-sm font-medium text-gray-700 mb-3">Diagnosis</h4>
					<div className="space-y-2">
						{diagnoses.map((diagnosis, i) => (
							<div
								key={i}
								className={`flex items-start space-x-2 p-3 rounded-lg ${
									diagnosis.includes("healthy") || diagnosis.includes("consistent")
										? "bg-green-50"
										: diagnosis.includes("Weak") ||
											  diagnosis.includes("dropped") ||
											  diagnosis.includes("Slow")
											? "bg-red-50"
											: "bg-yellow-50"
								}`}
							>
								<AlertTriangle
									className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
										diagnosis.includes("healthy") || diagnosis.includes("consistent")
											? "text-green-600"
											: diagnosis.includes("Weak") ||
												  diagnosis.includes("dropped") ||
												  diagnosis.includes("Slow")
												? "text-red-600"
												: "text-yellow-600"
									}`}
								/>
								<p
									className={`text-sm ${
										diagnosis.includes("healthy") || diagnosis.includes("consistent")
											? "text-green-800"
											: diagnosis.includes("Weak") ||
												  diagnosis.includes("dropped") ||
												  diagnosis.includes("Slow")
												? "text-red-800"
												: "text-yellow-800"
									}`}
								>
									{diagnosis}
								</p>
							</div>
						))}
					</div>
				</div>

				{/* Statistics Table */}
				{device.minute_stats && device.minute_stats.length > 0 && (
					<div>
						<h4 className="text-sm font-medium text-gray-700 mb-3">
							Per-Minute Statistics
						</h4>
						<div className="overflow-x-auto">
							<table className="min-w-full divide-y divide-gray-200 text-sm">
								<thead className="bg-gray-50">
									<tr>
										<th className="px-3 py-2 text-left text-xs font-medium text-gray-500 uppercase">
											Minute
										</th>
										<th className="px-3 py-2 text-left text-xs font-medium text-gray-500 uppercase">
											Samples
										</th>
										<th className="px-3 py-2 text-left text-xs font-medium text-gray-500 uppercase">
											Download
										</th>
										<th className="px-3 py-2 text-left text-xs font-medium text-gray-500 uppercase">
											Std Dev
										</th>
										<th className="px-3 py-2 text-left text-xs font-medium text-gray-500 uppercase">
											Upload
										</th>
									</tr>
								</thead>
								<tbody className="divide-y divide-gray-200">
									{device.minute_stats
										.sort((a, b) => a.minute - b.minute)
										.map((stat) => (
											<tr key={stat.minute}>
												<td className="px-3 py-2 text-gray-900">{stat.minute}</td>
												<td className="px-3 py-2 text-gray-600">
													{stat.sample_count}
												</td>
												<td className="px-3 py-2 font-mono text-gray-900">
													{stat.download.average_mbps.toFixed(2)} Mbps
												</td>
												<td className="px-3 py-2 font-mono text-gray-600">
													{stat.download.std_dev.toFixed(2)}
												</td>
												<td className="px-3 py-2 font-mono text-gray-900">
													{stat.upload
														? `${stat.upload.average_mbps.toFixed(2)} Mbps`
														: "-"}
												</td>
											</tr>
										))}
								</tbody>
							</table>
						</div>
					</div>
				)}
			</div>
		</div>
	);
}
