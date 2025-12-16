"use client";

import {
	ArrowLeft,
	ChevronRight,
	GitBranch,
	Globe,
	MapPin,
	Router,
	Smartphone,
	Tag,
	Tags,
	Wifi,
	WifiOff,
	XCircle,
} from "lucide-react";
import dynamic from "next/dynamic";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useGetDeviceInfo } from "@/app/api-client";
import DeviceHeader from "./DeviceHeader";

const LocationMap = dynamic(() => import("./LocationMap"), {
	ssr: false,
	loading: () => (
		<div className="h-64 bg-gray-100 rounded-lg flex items-center justify-center">
			Loading map...
		</div>
	),
});

const DeviceDetailPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const { data: device, isLoading: loading } = useGetDeviceInfo(serial);

	const getFlagUrl = (countryCode: string) => {
		return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
	};

	if (loading) {
		return (
			<div className="space-y-6">
				{/* Breadcrumb Skeleton */}
				<div className="flex items-center space-x-2">
					<div className="h-4 bg-gray-200 rounded w-16 animate-pulse" />
					<ChevronRight className="w-4 h-4 text-gray-300" />
					<div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
				</div>

				{/* Header Skeleton */}
				<div className="bg-white rounded-lg border border-gray-200 p-6">
					<div className="flex items-start justify-between">
						<div className="flex items-center space-x-4">
							<div className="p-3 bg-gray-100 rounded-lg">
								<div className="w-8 h-8 bg-gray-200 rounded animate-pulse" />
							</div>
							<div>
								<div className="h-8 bg-gray-200 rounded w-48 animate-pulse mb-2" />
								<div className="h-4 bg-gray-200 rounded w-32 animate-pulse mb-1" />
								<div className="h-4 bg-gray-200 rounded w-24 animate-pulse" />
							</div>
						</div>
						<div className="text-right">
							<div className="h-4 bg-gray-200 rounded w-16 animate-pulse mb-1" />
							<div className="h-4 bg-gray-200 rounded w-20 animate-pulse" />
						</div>
					</div>
				</div>
			</div>
		);
	}

	if (!device) {
		return (
			<div className="space-y-6">
				<div className="text-center py-12">
					<XCircle className="w-12 h-12 text-gray-400 mx-auto mb-4" />
					<h3 className="text-lg font-medium text-gray-900 mb-2">
						Device not found
					</h3>
					<p className="text-gray-500">
						The device with serial number "{serial}" could not be found.
					</p>
				</div>
			</div>
		);
	}

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
			<DeviceHeader device={device} serial={serial} />

			{/* Tabs */}
			<div className="border-b border-gray-200">
				<nav className="-mb-px flex space-x-8">
					<button className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm">
						Overview
					</button>
					<Link
						href={`/devices/${serial}/commands`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Commands
					</Link>
				</nav>
			</div>

			{/* Overview Content */}
			<div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
				{/* System Information */}
				<div className="bg-white rounded-lg border border-gray-200 p-6">
					<h3 className="text-lg font-semibold text-gray-900 mb-4">
						System Information
					</h3>

					{/* Labels */}
					{device.labels && Object.keys(device.labels).length > 0 && (
						<>
							<div className="flex items-center space-x-2 mb-3">
								<Tags className="w-4 h-4 text-purple-600" />
								<span className="text-gray-700 font-medium">Labels</span>
							</div>
							<div className="space-y-2 mb-4">
								{Object.entries(device.labels).map(([key, value]) => (
									<div
										key={key}
										className="flex items-center p-2 hover:bg-gray-50 rounded"
									>
										<span className="text-gray-700 font-mono text-sm min-w-fit mr-3">
											{key}
										</span>
										<div className="flex-1 border-b border-dotted border-gray-300"></div>
										<span className="text-gray-900 font-mono text-sm ml-3">
											{value}
										</span>
									</div>
								))}
							</div>
							<hr className="border-gray-200 mb-4" />
						</>
					)}

					{/* System Info Details */}
					<div className="space-y-3">
						{/* Device Model */}
						{device.system_info?.device_tree?.model && (
							<div className="flex justify-between">
								<span className="text-gray-700">Model</span>
								<span className="text-gray-900">
									{device.system_info.device_tree.model}
								</span>
							</div>
						)}

						{/* Operating System */}
						{device.system_info?.os_release?.pretty_name && (
							<div className="flex justify-between">
								<span className="text-gray-700">Operating System</span>
								<span className="font-mono text-sm text-gray-900">
									{device.system_info.os_release.pretty_name}
								</span>
							</div>
						)}

						{/* Kernel Version */}
						{device.system_info?.proc?.version && (
							<div className="flex justify-between">
								<span className="text-gray-700">Kernel</span>
								<span className="font-mono text-sm text-gray-900">
									{device.system_info.proc.version}
								</span>
							</div>
						)}

						{/* Distribution */}
						{device.release && (
							<div className="flex justify-between">
								<span className="text-gray-700 flex items-center">
									<GitBranch className="w-4 h-4 text-gray-400 mr-2" />
									Distribution
								</span>
								<Link
									href={`/distributions/${device.release?.distribution_id}`}
									className="block font-mono text-sm text-blue-600 hover:text-blue-800 hover:underline cursor-pointer transition-colors"
								>
									{device.release.distribution_name}
								</Link>
							</div>
						)}

						{/* Current Release */}
						{device.release && (
							<div className="flex justify-between">
								<span className="text-gray-700 flex items-center">
									<Tag className="w-4 h-4 text-gray-400 mr-2" />
									Current Release
								</span>
								<Link
									href={`/releases/${device.release?.id}`}
									className="block font-mono text-sm text-blue-600 hover:text-blue-800 hover:underline cursor-pointer transition-colors"
								>
									{device.release.version}
								</Link>
							</div>
						)}

						{/* Target Distribution */}
						{device.target_release &&
							device.target_release_id !== device.release_id && (
								<>
									<div className="flex justify-between">
										<span className="text-gray-700 flex items-center">
											<GitBranch className="w-4 h-4 text-purple-400 mr-2" />
											Target Distribution
										</span>
										<span className="font-mono text-sm text-gray-900">
											{device.target_release.distribution_name}
										</span>
									</div>

									{/* Target Release */}
									<div className="flex justify-between">
										<span className="text-gray-700 flex items-center">
											<Tag className="w-4 h-4 text-purple-400 mr-2" />
											Target Release
										</span>
										<span className="font-mono text-sm text-gray-900">
											{device.target_release.version}
										</span>
									</div>
								</>
							)}

						{/* Agent Version */}
						{device.system_info?.smith?.version && (
							<div className="flex justify-between">
								<span className="text-gray-700">Agent</span>
								<span className="font-mono text-sm text-gray-900">
									{device.system_info.smith.version}
								</span>
							</div>
						)}

						{/* Boot Time */}
						{device.system_info?.proc?.stat?.btime && (
							<div className="flex justify-between">
								<span className="text-gray-700">Boot Time</span>
								<span className="text-sm text-gray-900">
									{new Date(
										device.system_info.proc.stat.btime * 1000,
									).toLocaleString()}
								</span>
							</div>
						)}

						{/* Registration Date */}
						{device.created_on && (
							<div className="flex justify-between">
								<span className="text-gray-700">Registration Date</span>
								<span className="text-sm text-gray-900">
									{new Date(device.created_on).toLocaleString()}
								</span>
							</div>
						)}
					</div>
				</div>

				{/* Network Connections */}
				<div className="bg-white rounded-lg border border-gray-200 p-6">
					<h3 className="text-lg font-semibold text-gray-900 mb-4">
						Network Connections
					</h3>
					{device.system_info?.network?.interfaces ? (
						(() => {
							const activeConnections = Object.entries(
								device.system_info.network.interfaces,
							).filter(([name]) => {
								const connectionStatus =
									device.system_info?.connection_statuses?.find(
										(conn) => conn.device_name === name,
									);
								return connectionStatus?.connection_state === "connected";
							});

							const inactiveConnections = Object.entries(
								device.system_info.network.interfaces,
							).filter(([name]) => {
								const connectionStatus =
									device.system_info?.connection_statuses?.find(
										(conn) => conn.device_name === name,
									);
								return connectionStatus?.connection_state !== "connected";
							});

							if (
								activeConnections.length === 0 &&
								inactiveConnections.length === 0
							) {
								return (
									<div className="flex items-center text-gray-500 text-sm">
										<WifiOff className="w-4 h-4 mr-2" />
										No network interfaces found
									</div>
								);
							}

							return (
								<div className="space-y-3">
									{/* Active Connections */}
									{activeConnections.map(([name, iface]) => {
										const connectionStatus =
											device.system_info?.connection_statuses?.find(
												(conn) => conn.device_name === name,
											);
										const deviceType =
											connectionStatus?.device_type || "unknown";
										const primaryIP = iface.ips[0];

										return (
											<div
												key={name}
												className="p-3 border border-green-200 bg-green-50 rounded"
											>
												<div className="flex items-center justify-between mb-2">
													<div className="flex items-center space-x-2">
														{deviceType === "wifi" ? (
															<Wifi className="w-4 h-4 text-green-600" />
														) : deviceType === "ethernet" ? (
															<Router className="w-4 h-4 text-blue-600" />
														) : (
															<Smartphone className="w-4 h-4 text-gray-600" />
														)}
														<span className="font-mono text-sm font-medium text-gray-900">
															{name}
														</span>
														<div className="w-2 h-2 rounded-full bg-green-500"></div>
													</div>
													<span className="text-xs text-green-600 font-medium">
														Connected
													</span>
												</div>

												<div className="space-y-2 text-sm">
													{primaryIP && (
														<div className="flex justify-between">
															<span className="text-gray-600">Primary IP</span>
															<span className="font-mono text-gray-900">
																{primaryIP}
															</span>
														</div>
													)}
													<div className="flex justify-between">
														<span className="text-gray-600">MAC Address</span>
														<span className="font-mono text-gray-900">
															{iface.mac_address}
														</span>
													</div>
													{iface.ips.length > 1 && (
														<div className="flex justify-between">
															<span className="text-gray-600">
																Additional IPs
															</span>
															<div className="text-right">
																{iface.ips.slice(1).map((ip, index) => (
																	<div
																		key={index}
																		className="font-mono text-gray-900"
																	>
																		{ip}
																	</div>
																))}
															</div>
														</div>
													)}
												</div>
											</div>
										);
									})}

									{/* Show inactive connections if any */}
									{inactiveConnections.length > 0 && (
										<details className="mt-3">
											<summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800">
												Show inactive connections ({inactiveConnections.length})
											</summary>
											<div className="mt-2 space-y-2">
												{inactiveConnections.map(([name, iface]) => {
													const connectionStatus =
														device.system_info?.connection_statuses?.find(
															(conn) => conn.device_name === name,
														);
													const deviceType =
														connectionStatus?.device_type || "unknown";
													const primaryIP = iface.ips[0];

													return (
														<div
															key={name}
															className="p-3 border border-gray-200 bg-gray-50 rounded"
														>
															<div className="flex items-center justify-between mb-2">
																<div className="flex items-center space-x-2">
																	{deviceType === "wifi" ? (
																		<WifiOff className="w-4 h-4 text-gray-400" />
																	) : deviceType === "ethernet" ? (
																		<Router className="w-4 h-4 text-gray-400" />
																	) : (
																		<Smartphone className="w-4 h-4 text-gray-400" />
																	)}
																	<span className="font-mono text-sm font-medium text-gray-900">
																		{name}
																	</span>
																	<div className="w-2 h-2 rounded-full bg-gray-400"></div>
																</div>
																<span className="text-xs text-gray-500 font-medium">
																	Disconnected
																</span>
															</div>

															<div className="space-y-2 text-sm">
																{primaryIP && (
																	<div className="flex justify-between">
																		<span className="text-gray-600">
																			Primary IP
																		</span>
																		<span className="font-mono text-gray-900">
																			{primaryIP}
																		</span>
																	</div>
																)}
																<div className="flex justify-between">
																	<span className="text-gray-600">
																		MAC Address
																	</span>
																	<span className="font-mono text-gray-900">
																		{iface.mac_address}
																	</span>
																</div>
																{iface.ips.length > 1 && (
																	<div className="flex justify-between">
																		<span className="text-gray-600">
																			Additional IPs
																		</span>
																		<div className="text-right">
																			{iface.ips.slice(1).map((ip) => (
																				<div
																					key={ip}
																					className="font-mono text-gray-900"
																				>
																					{ip}
																				</div>
																			))}
																		</div>
																	</div>
																)}
															</div>
														</div>
													);
												})}
											</div>
										</details>
									)}
								</div>
							);
						})()
					) : (
						<p className="text-gray-500 text-sm">
							No network interface information available
						</p>
					)}
				</div>

				{/* Location Information */}
				<div className="bg-white rounded-lg border border-gray-200 p-6">
					<div className="flex items-center space-x-2 mb-4">
						<MapPin className="w-5 h-5 text-blue-600" />
						<h3 className="text-lg font-semibold text-gray-900">
							Location Information
						</h3>
					</div>

					{device.ip_address ? (
						<div className="space-y-4">
							{/* Map */}
							<LocationMap
								countryCode={device.ip_address.country_code}
								city={device.ip_address.city}
								country={device.ip_address.country}
							/>

							{/* Location Details */}
							<div className="space-y-3">
								<div className="flex items-center space-x-3">
									<Globe className="w-4 h-4 text-gray-500" />
									<span className="font-mono text-sm text-gray-900">
										{device.ip_address.ip_address}
									</span>
									{device.ip_address.country_code && (
										<img
											src={getFlagUrl(device.ip_address.country_code)}
											alt={device.ip_address.country || "Country flag"}
											className="w-6 h-4 rounded-sm border border-gray-200"
											onError={(e) => {
												(e.target as HTMLImageElement).style.display = "none";
											}}
										/>
									)}
								</div>

								{device.ip_address.name && (
									<div className="flex items-center justify-between">
										<span className="text-gray-600">Location Name</span>
										<span className="text-gray-900 font-medium">
											{device.ip_address.name}
										</span>
									</div>
								)}
								{device.ip_address.country && (
									<div className="flex items-center justify-between">
										<span className="text-gray-600">Country</span>
										<span className="text-gray-900 font-medium">
											{device.ip_address.country}
											{device.ip_address.country_code &&
												` (${device.ip_address.country_code})`}
										</span>
									</div>
								)}
								{device.ip_address.region && (
									<div className="flex items-center justify-between">
										<span className="text-gray-600">Region</span>
										<span className="text-gray-900 font-medium">
											{device.ip_address.region}
										</span>
									</div>
								)}
								{device.ip_address.city && (
									<div className="flex items-center justify-between">
										<span className="text-gray-600">City</span>
										<span className="text-gray-900 font-medium">
											{device.ip_address.city}
										</span>
									</div>
								)}
								{device.ip_address.isp && (
									<div className="flex items-center justify-between">
										<span className="text-gray-600">Internet Provider</span>
										<span className="text-gray-900 font-medium">
											{device.ip_address.isp}
										</span>
									</div>
								)}
								{device.ip_address.coordinates && (
									<div className="flex items-center justify-between">
										<span className="text-gray-600">Coordinates</span>
										<span className="font-mono text-sm text-gray-900">
											{device.ip_address.coordinates[0].toFixed(4)},{" "}
											{device.ip_address.coordinates[1].toFixed(4)}
										</span>
									</div>
								)}
							</div>
						</div>
					) : (
						<div className="flex items-center justify-center py-8">
							<div className="text-center">
								<Globe className="w-12 h-12 text-gray-300 mx-auto mb-4" />
								<p className="text-gray-500">
									No location information available
								</p>
								<p className="text-gray-400 text-sm mt-1">
									This device has no associated IP address data
								</p>
							</div>
						</div>
					)}
				</div>
			</div>
		</div>
	);
};

export default DeviceDetailPage;
