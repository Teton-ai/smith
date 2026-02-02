"use client";

import { ArrowLeft, FileText, Play } from "lucide-react";
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

	const loading = deviceLoading || servicesLoading;

	const handleViewLogs = (serviceName: string) => {
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
				<div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
					{/* Services List */}
					<div className="bg-white rounded-lg border border-gray-200 p-6">
						<h3 className="text-lg font-semibold text-gray-900 mb-4">
							Services ({services.length})
						</h3>
						<div className="space-y-3">
							{services.map((service: DeviceService) => (
								<div
									key={service.id}
									className={`p-4 border rounded-lg transition-colors ${
										selectedService === service.service_name
											? "border-blue-500 bg-blue-50"
											: "border-gray-200 hover:border-gray-300"
									}`}
								>
									<div className="flex items-center justify-between">
										<div>
											<div className="font-mono text-sm font-medium text-gray-900">
												{service.service_name}
											</div>
											{service.watchdog_sec && (
												<div className="text-xs text-gray-500 mt-1">
													Watchdog: {service.watchdog_sec}s
												</div>
											)}
										</div>
										<button
											onClick={() => handleViewLogs(service.service_name)}
											className="flex items-center space-x-2 px-3 py-1.5 text-sm font-medium text-blue-600 hover:text-blue-800 hover:bg-blue-100 rounded transition-colors"
										>
											<Play className="w-4 h-4" />
											<span>Stream Logs</span>
										</button>
									</div>
								</div>
							))}
						</div>
					</div>

					{/* Log Viewer */}
					<div className="bg-white rounded-lg border border-gray-200 p-6">
						{selectedService ? (
							<>
								<div className="flex items-center justify-between mb-4">
									<h3 className="text-lg font-semibold text-gray-900">
										Logs: {selectedService}
									</h3>
									<button
										onClick={handleCloseLogs}
										className="px-3 py-1.5 text-sm font-medium text-gray-600 hover:text-gray-800 hover:bg-gray-100 rounded transition-colors"
									>
										Close
									</button>
								</div>
								<LogViewer
									key={selectedService}
									deviceSerial={serial}
									serviceName={selectedService}
								/>
							</>
						) : (
							<div className="text-center py-12">
								<FileText className="w-12 h-12 text-gray-300 mx-auto mb-4" />
								<p className="text-gray-500">
									Select a service to stream its logs
								</p>
							</div>
						)}
					</div>
				</div>
			)}
		</div>
	);
};

export default ServicesPage;
