import {
	Badge,
	Card,
	CountryFlag,
	InfoRow,
	LabelChip,
	PageContainer,
	Panel,
	SECTION_THEMES,
} from "@teton/smith-ui";
import {
	Cpu,
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
import { lazy, Suspense } from "react";
import { Link, useParams } from "react-router";
import { useGetDeviceInfo } from "@/app/api-client";
import { DeviceDetailLayout } from "./DeviceDetailLayout";
import DeviceVariables from "./DeviceVariables";
import SecurityAudit from "./SecurityAudit";
import WifiPanel from "./WifiPanel";

const LocationMap = lazy(() => import("./LocationMap"));

const MapFallback = () => (
	<div className="h-64 bg-gray-100 rounded-lg flex items-center justify-center">
		Loading map...
	</div>
);

const linkClass =
	"font-mono text-sm text-blue-600 hover:text-blue-800 hover:underline cursor-pointer transition-colors";

const DeviceDetailPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const { data: device, isLoading: loading } = useGetDeviceInfo(serial);

	if (loading) {
		return (
			<PageContainer>
				<div className="h-4 w-40 bg-gray-200 rounded animate-pulse" />
				<Card className="p-6">
					<div className="flex items-center space-x-4">
						<div className="p-3 bg-gray-100 rounded-lg">
							<div className="w-8 h-8 bg-gray-200 rounded animate-pulse" />
						</div>
						<div className="space-y-2">
							<div className="h-8 bg-gray-200 rounded w-48 animate-pulse" />
							<div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
							<div className="h-4 bg-gray-200 rounded w-24 animate-pulse" />
						</div>
					</div>
				</Card>
			</PageContainer>
		);
	}

	if (!device) {
		return (
			<PageContainer>
				<div className="text-center py-12">
					<XCircle className="w-12 h-12 text-gray-400 mx-auto mb-4" />
					<h3 className="text-lg font-medium text-gray-900 mb-2">
						Device not found
					</h3>
					<p className="text-gray-500">
						The device with serial number "{serial}" could not be found.
					</p>
				</div>
			</PageContainer>
		);
	}

	return (
		<DeviceDetailLayout serial={serial} device={device} activeTab="overview">
			{/* Overview Content */}
			<div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
				{/* System Information */}
				<Panel
					title="System Information"
					icon={Cpu}
					theme={SECTION_THEMES.blue}
				>
					{/* Labels */}
					{device.labels && Object.keys(device.labels).length > 0 && (
						<div className="mb-4 pb-4 border-b border-gray-100">
							<div className="flex items-center gap-2 mb-2.5">
								<Tags className="w-3.5 h-3.5 text-gray-400" />
								<span className="text-xs font-semibold uppercase tracking-wide text-gray-500">
									Labels
								</span>
							</div>
							<div className="flex flex-wrap gap-1.5">
								{Object.entries(device.labels).map(([key, value]) => (
									<LabelChip key={key} name={key} value={value} />
								))}
							</div>
						</div>
					)}

					{/* System Info Details */}
					<div className="space-y-3">
						{device.system_info?.device_tree?.model && (
							<InfoRow label="Model">
								{device.system_info.device_tree.model}
							</InfoRow>
						)}

						{device.system_info?.os_release?.pretty_name && (
							<InfoRow label="Operating System">
								<span className="font-mono">
									{device.system_info.os_release.pretty_name}
								</span>
							</InfoRow>
						)}

						{device.system_info?.proc?.version && (
							<InfoRow label="Kernel">
								<span className="font-mono">
									{device.system_info.proc.version}
								</span>
							</InfoRow>
						)}

						{device.release && (
							<InfoRow label="Distribution" icon={GitBranch}>
								<Link
									to={`/distributions/${device.release?.distribution_id}`}
									className={linkClass}
								>
									{device.release.distribution_name}
								</Link>
							</InfoRow>
						)}

						{device.release && (
							<InfoRow label="Current Release" icon={Tag}>
								<Link
									to={`/releases/${device.release?.id}`}
									className={linkClass}
								>
									{device.release.version}
								</Link>
							</InfoRow>
						)}

						{device.target_release &&
							device.target_release_id !== device.release_id && (
								<>
									<InfoRow
										label="Target Distribution"
										icon={GitBranch}
										iconClassName="text-purple-400"
									>
										<Link
											to={`/distributions/${device.target_release.distribution_id}`}
											className={linkClass}
										>
											{device.target_release.distribution_name}
										</Link>
									</InfoRow>

									<InfoRow
										label="Target Release"
										icon={Tag}
										iconClassName="text-purple-400"
									>
										<Link
											to={`/releases/${device.target_release?.id}`}
											className={linkClass}
										>
											{device.target_release.version}
										</Link>
									</InfoRow>
								</>
							)}

						{device.system_info?.smith?.version && (
							<InfoRow label="Agent">
								<span className="font-mono">
									{device.system_info.smith.version}
								</span>
							</InfoRow>
						)}

						{device.system_info?.proc?.stat?.btime && (
							<InfoRow label="Boot Time">
								{new Date(
									device.system_info.proc.stat.btime * 1000,
								).toLocaleString()}
							</InfoRow>
						)}

						{device.created_on && (
							<InfoRow label="Registration Date">
								{new Date(device.created_on).toLocaleString()}
							</InfoRow>
						)}
					</div>
				</Panel>

				{/* Network Connections */}
				<Panel
					title="Network Connections"
					icon={Wifi}
					theme={SECTION_THEMES.green}
				>
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
												className="p-3 border border-green-200 bg-green-50 rounded-lg"
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
													</div>
													<Badge variant="green" pill>
														Connected
													</Badge>
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
															className="p-3 border border-gray-200 bg-gray-50 rounded-lg"
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
																</div>
																<Badge variant="gray" pill>
																	Disconnected
																</Badge>
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
				</Panel>

				{/* Variables (secrets) */}
				<DeviceVariables deviceId={device.id} />
			</div>

			{/* WiFi */}
			<WifiPanel serial={serial} />

			{/* Security Audit (left) and Location Information (right) */}
			<div className="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
				<SecurityAudit serial={serial} deviceId={device.id} />

				{/* Location Information */}
				<Panel
					title="Location Information"
					icon={MapPin}
					theme={SECTION_THEMES.purple}
				>
					{device.ip_address ? (
						<div className="space-y-4">
							{/* Map */}
							<Suspense fallback={<MapFallback />}>
								<LocationMap
									countryCode={device.ip_address.country_code}
									city={device.ip_address.city}
									country={device.ip_address.country}
								/>
							</Suspense>

							{/* Location Details */}
							<div className="space-y-3">
								<div className="flex items-center space-x-3">
									<Globe className="w-4 h-4 text-gray-500" />
									<span className="font-mono text-sm text-gray-900">
										{device.ip_address.ip_address}
									</span>
									<CountryFlag
										countryCode={device.ip_address.country_code}
										country={device.ip_address.country}
									/>
								</div>

								{device.ip_address.name && (
									<InfoRow label="Location Name">
										<span className="font-medium">
											{device.ip_address.name}
										</span>
									</InfoRow>
								)}
								{device.ip_address.country && (
									<InfoRow label="Country">
										<span className="font-medium">
											{device.ip_address.country}
											{device.ip_address.country_code &&
												` (${device.ip_address.country_code})`}
										</span>
									</InfoRow>
								)}
								{device.ip_address.region && (
									<InfoRow label="Region">
										<span className="font-medium">
											{device.ip_address.region}
										</span>
									</InfoRow>
								)}
								{device.ip_address.city && (
									<InfoRow label="City">
										<span className="font-medium">
											{device.ip_address.city}
										</span>
									</InfoRow>
								)}
								{device.ip_address.isp && (
									<InfoRow label="Internet Provider">
										<span className="font-medium">{device.ip_address.isp}</span>
									</InfoRow>
								)}
								{device.ip_address.coordinates && (
									<InfoRow label="Coordinates">
										<span className="font-mono">
											{device.ip_address.coordinates[0].toFixed(4)},{" "}
											{device.ip_address.coordinates[1].toFixed(4)}
										</span>
									</InfoRow>
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
				</Panel>
			</div>
		</DeviceDetailLayout>
	);
};

export default DeviceDetailPage;
