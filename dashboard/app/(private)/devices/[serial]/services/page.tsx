"use client";

import { ArrowLeft, FileText, Play, Radio, X } from "lucide-react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useState } from "react";
import { useGetDeviceInfo } from "@/app/api-client";
import DeviceHeader from "../DeviceHeader";
import LogViewer from "./LogViewer";
import { type DeviceService, useDeviceServices } from "./useDeviceServices";

const ServicesPage = () => {
	const params = useParams();
	const serial = params.serial as string;

	const { data: device, isLoading: deviceLoading } = useGetDeviceInfo(serial);
	const { data: services, isLoading: servicesLoading } =
		useDeviceServices(serial);

	const [selectedService, setSelectedService] = useState<string | null>(null);
	const [connectionStatus, setConnectionStatus] = useState<
		"connecting" | "connected" | "disconnected"
	>("connecting");

	const loading = deviceLoading || servicesLoading;

	const handleSelectService = (serviceName: string) => {
		setConnectionStatus("connecting");
		setSelectedService(serviceName);
	};

	const handleCloseLogs = () => {
		setSelectedService(null);
	};

	return (
		<div className="space-y-6">
			{/* Header with Back Button */}
			<div className="flex items-center space-x-4">
				<Link
					href="/devices"
					className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
				>
					<ArrowLeft className="w-4 h-4" />
					<span className="text-sm font-medium">Back to Devices</span>
				</Link>
			</div>

			{/* Device Header */}
			{device && <DeviceHeader device={device} serial={serial} />}

			{/* Tabs */}
			<div className="border-b border-gray-200">
				<nav className="-mb-px flex space-x-8">
					<Link
						href={`/devices/${serial}`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Overview
					</Link>
					<Link
						href={`/devices/${serial}/commands`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Commands
					</Link>
					<button className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm">
						Services
					</button>
				</nav>
			</div>

			{/* Services Content */}
			{loading ? (
				<div className="p-6 text-gray-500">Loading services...</div>
			) : !services || services.length === 0 ? (
				<div className="bg-white rounded-lg border border-gray-200 p-6">
					<div className="text-center py-8">
						<FileText className="w-12 h-12 text-gray-300 mx-auto mb-4" />
						<p className="text-gray-500">No services found for this device</p>
						<p className="text-gray-400 text-sm mt-1">
							Services are extracted from packages in the device's release
						</p>
					</div>
				</div>
			) : (
				<div className="flex gap-4">
					{/* Services List - Compact Sidebar */}
					<div className="w-64 flex-shrink-0">
						<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
							<div className="px-4 py-3 border-b border-gray-100 bg-gray-50">
								<span className="text-sm font-medium text-gray-700">
									Services
								</span>
								<span className="ml-2 text-xs text-gray-500">
									{services.length}
								</span>
							</div>
							<div className="divide-y divide-gray-100">
								{services.map((service: DeviceService) => {
									const healthColor = service.active_state
										? service.active_state === "active" &&
											service.n_restarts === 0
											? "bg-green-500"
											: "bg-red-500"
										: "bg-gray-300";
									const healthTooltip = service.active_state
										? `${service.active_state}, restarts: ${service.n_restarts}${service.checked_at ? `, checked: ${new Date(service.checked_at).toLocaleString()}` : ""}`
										: "No health data";
									return (
										<div
											key={service.id}
											className={`flex items-center justify-between px-4 py-2.5 ${
												selectedService === service.service_name
													? "bg-blue-50 border-l-2 border-l-blue-500"
													: "border-l-2 border-l-transparent"
											}`}
										>
											<div className="flex items-center gap-2 min-w-0">
												<span
													className={`w-2 h-2 rounded-full flex-shrink-0 ${healthColor}`}
													title={healthTooltip}
												/>
												<div className="min-w-0">
													<div className="font-mono text-sm text-gray-900 truncate">
														{service.service_name}
													</div>
													{service.watchdog_sec && (
														<div className="text-xs text-gray-400 mt-0.5">
															Watchdog: {service.watchdog_sec}s
														</div>
													)}
												</div>
											</div>
											<button
												onClick={() =>
													handleSelectService(service.service_name)
												}
												className="flex-shrink-0 p-1.5 rounded text-gray-400 hover:text-blue-500 hover:bg-blue-100 transition-colors cursor-pointer"
												title="Stream logs"
											>
												<Play className="w-3.5 h-3.5" />
											</button>
										</div>
									);
								})}
							</div>
						</div>
					</div>

					{/* Log Viewer - Main Content */}
					<div className="flex-1 min-w-0">
						<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
							{selectedService ? (
								<>
									<div className="flex items-center justify-between px-4 py-3 border-b border-gray-100 bg-gray-50">
										<div className="flex items-center gap-2">
											<div
												className={`w-2 h-2 rounded-full ${
													connectionStatus === "connecting"
														? "bg-yellow-400 animate-pulse"
														: connectionStatus === "connected"
															? "bg-green-500"
															: "bg-gray-400"
												}`}
											/>
											<span className="font-mono text-sm font-medium text-gray-900">
												{selectedService}
											</span>
										</div>
										<button
											onClick={handleCloseLogs}
											className="p-1 text-gray-400 hover:text-gray-600 hover:bg-gray-200 rounded transition-colors cursor-pointer"
										>
											<X className="w-4 h-4" />
										</button>
									</div>
									<LogViewer
										key={selectedService}
										deviceSerial={serial}
										serviceName={selectedService}
										onStatusChange={setConnectionStatus}
									/>
								</>
							) : (
								<div className="flex items-center justify-center h-96 text-gray-400">
									<div className="text-center">
										<Radio className="w-8 h-8 mx-auto mb-2 opacity-50" />
										<p className="text-sm">Select a service to stream logs</p>
									</div>
								</div>
							)}
						</div>
					</div>
				</div>
			)}
		</div>
	);
};

export default ServicesPage;
