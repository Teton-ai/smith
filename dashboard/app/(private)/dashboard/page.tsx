"use client";

import {
	AlertTriangle,
	CheckCircle,
	ChevronRight,
	Clock,
	Cpu,
	Package,
	XCircle,
} from "lucide-react";
import Link from "next/link";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import { useConfig } from "@/app/hooks/config";
import {
	type Device,
	useGetDashboard,
	useGetDevices,
} from "../../api-client";

const AdminPanel = () => {
	const { config } = useConfig();

	// Build exclude_labels query param
	const excludeLabels =
		config?.DASHBOARD_EXCLUDED_LABELS?.split(",")
			.map((l) => l.trim())
			.filter(Boolean) || [];

	const dashboardQuery = useGetDashboard({
		query: { refetchInterval: 5000 },
	});

	const { data: outdatedDevices = [], isLoading: outdatedLoading } =
		useGetDevices(
			{
				outdated: true,
				outdated_minutes: 30,
				exclude_labels: excludeLabels,
			},
			{ query: { refetchInterval: 5000 } },
		);

	const { data: offlineDevices = [], isLoading: offlineLoading } =
		useGetDevices(
			{
				online: false,
				exclude_labels: excludeLabels,
			},
			{ query: { refetchInterval: 5000 } },
		);

	const loading =
		dashboardQuery.isLoading ||
		outdatedLoading ||
		offlineLoading;

	const getDeviceName = (device: Device) => device.serial_number;

	const formatTimeAgo = (dateString: string) => {
		const now = new Date();
		const past = new Date(dateString);
		const diff = now.getTime() - past.getTime();
		const minutes = Math.floor(diff / 60000);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days}d ago`;
		if (hours > 0) return `${hours}h ago`;
		return `${minutes}m ago`;
	};

	const getFlagUrl = (countryCode: string) => {
		return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
	};

	// Sort by last_seen descending (most recent first), never seen at the end
	const sortByLastSeen = (a: Device, b: Device) => {
		if (!a.last_seen && !b.last_seen) return 0;
		if (!a.last_seen) return 1;
		if (!b.last_seen) return -1;
		return new Date(b.last_seen).getTime() - new Date(a.last_seen).getTime();
	};

	// Outdated devices (pending update) - backend filters by outdated_minutes=30
	const getOutdatedDuration = (device: Device) => {
		if (!device.target_release_id_set_at) return "";
		const setAt = new Date(device.target_release_id_set_at);
		const now = new Date();
		const diffMinutes = Math.floor(
			(now.getTime() - setAt.getTime()) / (1000 * 60),
		);
		const diffHours = Math.floor(diffMinutes / 60);
		const diffDays = Math.floor(diffHours / 24);

		if (diffDays > 0) return `${diffDays}d`;
		if (diffHours > 0) return `${diffHours}h`;
		return `${diffMinutes}m`;
	};

	const stuckUpdates = [...outdatedDevices].sort(sortByLastSeen);

	// Offline devices categorized by how long they've been offline
	const categorizeOffline = (devices: Device[]) => {
		const now = new Date();
		const recentlyOffline: Device[] = [];
		const offlineWeek: Device[] = [];
		const offlineMonth: Device[] = [];
		const neverSeen: Device[] = [];

		for (const device of devices) {
			if (!device.last_seen) {
				neverSeen.push(device);
				continue;
			}
			const lastSeen = new Date(device.last_seen);
			const diffDays =
				(now.getTime() - lastSeen.getTime()) / (1000 * 60 * 60 * 24);

			if (diffDays <= 1) {
				recentlyOffline.push(device);
			} else if (diffDays <= 7) {
				offlineWeek.push(device);
			} else if (diffDays <= 30) {
				offlineMonth.push(device);
			}
			// Devices offline > 30 days are not shown (considered abandoned)
		}

		return {
			recentlyOffline: recentlyOffline.sort(sortByLastSeen),
			offlineWeek: offlineWeek.sort(sortByLastSeen),
			offlineMonth: offlineMonth.sort(sortByLastSeen),
			neverSeen,
		};
	};

	const { recentlyOffline, offlineWeek, offlineMonth, neverSeen } =
		categorizeOffline(offlineDevices);
	const hasAttentionDevices =
		stuckUpdates.length > 0 ||
		recentlyOffline.length > 0 ||
		offlineWeek.length > 0 ||
		offlineMonth.length > 0 ||
		neverSeen.length > 0;

	return (
		<div className="space-y-6">
			{/* Overview Stats */}
					<div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
						{loading ? (
							// Skeleton loading for stats
							[...Array(2)].map((_, index) => (
								<div
									key={index}
									className="bg-white rounded-lg border border-gray-200 p-6"
								>
									<div className="flex items-center">
										<div className="w-8 h-8 bg-gray-200 rounded animate-pulse" />
										<div className="ml-4">
											<div className="h-4 bg-gray-200 rounded w-16 animate-pulse mb-2" />
											<div className="h-8 bg-gray-200 rounded w-12 animate-pulse" />
										</div>
									</div>
								</div>
							))
						) : (
							<>
								<div className="bg-white rounded-lg border border-gray-200 p-6">
									<div className="flex items-center">
										<CheckCircle className="w-8 h-8 text-green-500" />
										<div className="ml-4">
											<p className="text-sm font-medium text-gray-600">
												Online
											</p>
											<p className="text-2xl font-bold text-gray-900">
												{dashboardQuery.data?.online_count || 0}
											</p>
										</div>
									</div>
								</div>

								<div className="bg-white rounded-lg border border-gray-200 p-6">
									<div className="flex items-center">
										<Cpu className="w-8 h-8 text-blue-500" />
										<div className="ml-4">
											<p className="text-sm font-medium text-gray-600">
												Total Devices
											</p>
											<p className="text-2xl font-bold text-gray-900">
												{dashboardQuery.data?.total_count || 0}
											</p>
										</div>
									</div>
								</div>
							</>
						)}
					</div>

					{/* Device Status Sections */}
					{loading ? (
						<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
							{[...Array(4)].map((_, sectionIndex) => (
								<div
									key={sectionIndex}
									className="bg-white rounded-lg border border-gray-200"
								>
									<div className="px-4 py-3 border-b border-gray-200">
										<div className="h-5 bg-gray-200 rounded w-32 animate-pulse" />
									</div>
									<div className="divide-y divide-gray-200">
										{[...Array(3)].map((_, index) => (
											<div key={index} className="px-4 py-3">
												<div className="flex items-center justify-between">
													<div className="flex items-center space-x-3 flex-1">
														<div className="w-4 h-4 bg-gray-200 rounded animate-pulse" />
														<div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
													</div>
													<div className="h-4 bg-gray-200 rounded w-20 animate-pulse" />
												</div>
											</div>
										))}
									</div>
								</div>
							))}
						</div>
					) : hasAttentionDevices ? (
						<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
							{/* Pending Update Section */}
							{stuckUpdates.length > 0 && (
								<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
									<div className="bg-purple-50 px-4 py-3 border-b border-gray-200">
										<h4 className="text-sm font-semibold text-purple-800 flex items-center">
											<Package className="w-4 h-4 mr-2" />
											Pending Update ({stuckUpdates.length})
										</h4>
									</div>
									<div className="divide-y divide-gray-200">
										{stuckUpdates.slice(0, 10).map((device) => {
											const isOnline = device.last_seen
												? (Date.now() - new Date(device.last_seen).getTime()) /
														(1000 * 60) <=
													3
												: false;
											return (
												<Link
													key={device.id}
													className="block px-4 py-3 hover:bg-purple-50 transition-colors"
													href={`/devices/${device.serial_number}`}
												>
													<div className="flex items-center justify-between">
														<div className="flex items-center space-x-3 flex-1">
															{device.ip_address?.country_code && (
																<img
																	src={getFlagUrl(
																		device.ip_address.country_code,
																	)}
																	alt={
																		device.ip_address.country || "Country flag"
																	}
																	className="w-4 h-3 flex-shrink-0 rounded-sm"
																	onError={(e) => {
																		(
																			e.target as HTMLImageElement
																		).style.display = "none";
																	}}
																/>
															)}
															<span className="font-mono text-sm text-gray-900">
																{getDeviceName(device)}
															</span>
															{device.network?.network_score && (
																<NetworkQualityIndicator
																	isOnline={isOnline}
																	networkScore={device.network.network_score}
																/>
															)}
														</div>
														<div className="flex items-center space-x-3 text-sm">
															{device.release?.distribution_name && (
																<span className="text-gray-500">
																	{device.release.distribution_name}
																</span>
															)}
															<span className="text-purple-600 font-mono">
																{device.release?.version || device.release_id} →{" "}
																{device.target_release?.version ||
																	device.target_release_id}
															</span>
															{getOutdatedDuration(device) && (
																<span className="text-orange-600 text-xs font-medium">
																	{getOutdatedDuration(device)}
																</span>
															)}
															<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
														</div>
													</div>
												</Link>
											);
										})}
										{stuckUpdates.length > 10 && (
											<div className="px-4 py-3 bg-gray-50">
												<Link
													href="/devices?outdated=true"
													className="block text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
												>
													View all {stuckUpdates.length} devices →
												</Link>
											</div>
										)}
									</div>
								</div>
							)}

							{/* Recently Offline Section */}
							{recentlyOffline.length > 0 && (
								<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
									<div className="bg-yellow-50 px-4 py-3 border-b border-gray-200">
										<h4 className="text-sm font-semibold text-yellow-800 flex items-center">
											<Clock className="w-4 h-4 mr-2" />
											Recently Offline ({recentlyOffline.length})
										</h4>
									</div>
									<div className="divide-y divide-gray-200">
										{recentlyOffline.slice(0, 10).map((device) => {
											return (
												<Link
													key={device.id}
													className="block px-4 py-3 hover:bg-yellow-50 transition-colors"
													href={`/devices/${device.serial_number}`}
												>
													<div className="flex items-center justify-between">
														<div className="flex items-center space-x-3 flex-1">
															{device.ip_address?.country_code && (
																<img
																	src={getFlagUrl(
																		device.ip_address.country_code,
																	)}
																	alt={
																		device.ip_address.country || "Country flag"
																	}
																	className="w-4 h-3 flex-shrink-0 rounded-sm"
																	onError={(e) => {
																		(
																			e.target as HTMLImageElement
																		).style.display = "none";
																	}}
																/>
															)}
															<span className="font-mono text-sm text-gray-900">
																{getDeviceName(device)}
															</span>
														</div>
														<div className="flex items-center space-x-3 text-sm">
															<span className="text-gray-500">
																{device.last_seen
																	? formatTimeAgo(device.last_seen)
																	: "never"}
															</span>
															<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
														</div>
													</div>
												</Link>
											);
										})}
										{recentlyOffline.length > 10 && (
											<div className="px-4 py-3 bg-gray-50">
												<Link
													href="/devices"
													className="block text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
												>
													View all {recentlyOffline.length} devices →
												</Link>
											</div>
										)}
									</div>
								</div>
							)}

							{/* Offline This Week Section */}
							{offlineWeek.length > 0 && (
								<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
									<div className="bg-orange-50 px-4 py-3 border-b border-gray-200">
										<h4 className="text-sm font-semibold text-orange-800 flex items-center">
											<AlertTriangle className="w-4 h-4 mr-2" />
											Offline This Week ({offlineWeek.length})
										</h4>
									</div>
									<div className="divide-y divide-gray-200">
										{offlineWeek.slice(0, 10).map((device) => {
											return (
												<Link
													key={device.id}
													className="block px-4 py-3 hover:bg-orange-50 transition-colors"
													href={`/devices/${device.serial_number}`}
												>
													<div className="flex items-center justify-between">
														<div className="flex items-center space-x-3 flex-1">
															{device.ip_address?.country_code && (
																<img
																	src={getFlagUrl(
																		device.ip_address.country_code,
																	)}
																	alt={
																		device.ip_address.country || "Country flag"
																	}
																	className="w-4 h-3 flex-shrink-0 rounded-sm"
																	onError={(e) => {
																		(
																			e.target as HTMLImageElement
																		).style.display = "none";
																	}}
																/>
															)}
															<span className="font-mono text-sm text-gray-900">
																{getDeviceName(device)}
															</span>
														</div>
														<div className="flex items-center space-x-3 text-sm">
															<span className="text-gray-500">
																{device.last_seen
																	? formatTimeAgo(device.last_seen)
																	: "never"}
															</span>
															<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
														</div>
													</div>
												</Link>
											);
										})}
										{offlineWeek.length > 10 && (
											<div className="px-4 py-3 bg-gray-50">
												<Link
													href="/devices"
													className="block text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
												>
													View all {offlineWeek.length} devices →
												</Link>
											</div>
										)}
									</div>
								</div>
							)}

							{/* Long-term Offline Section */}
							{offlineMonth.length > 0 && (
								<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
									<div className="bg-red-50 px-4 py-3 border-b border-gray-200">
										<h4 className="text-sm font-semibold text-red-800 flex items-center">
											<XCircle className="w-4 h-4 mr-2" />
											Long-term Offline ({offlineMonth.length})
										</h4>
									</div>
									<div className="divide-y divide-gray-200">
										{offlineMonth.slice(0, 10).map((device) => {
											return (
												<Link
													key={device.id}
													className="block px-4 py-3 hover:bg-red-50 transition-colors"
													href={`/devices/${device.serial_number}`}
												>
													<div className="flex items-center justify-between">
														<div className="flex items-center space-x-3 flex-1">
															{device.ip_address?.country_code && (
																<img
																	src={getFlagUrl(
																		device.ip_address.country_code,
																	)}
																	alt={
																		device.ip_address.country || "Country flag"
																	}
																	className="w-4 h-3 flex-shrink-0 rounded-sm"
																	onError={(e) => {
																		(
																			e.target as HTMLImageElement
																		).style.display = "none";
																	}}
																/>
															)}
															<span className="font-mono text-sm text-gray-900">
																{getDeviceName(device)}
															</span>
														</div>
														<div className="flex items-center space-x-3 text-sm">
															<span className="text-gray-500">
																{device.last_seen
																	? formatTimeAgo(device.last_seen)
																	: "never"}
															</span>
															<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
														</div>
													</div>
												</Link>
											);
										})}
										{offlineMonth.length > 10 && (
											<div className="px-4 py-3 bg-gray-50">
												<Link
													href="/devices"
													className="block text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
												>
													View all {offlineMonth.length} devices →
												</Link>
											</div>
										)}
									</div>
								</div>
							)}

							{/* Never Connected Section */}
							{neverSeen.length > 0 && (
								<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
									<div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
										<h4 className="text-sm font-semibold text-gray-800 flex items-center">
											<AlertTriangle className="w-4 h-4 mr-2" />
											Never Connected ({neverSeen.length})
										</h4>
									</div>
									<div className="divide-y divide-gray-200">
										{neverSeen.slice(0, 10).map((device) => {
											return (
												<Link
													key={device.id}
													className="block px-4 py-3 hover:bg-gray-50 transition-colors"
													href={`/devices/${device.serial_number}`}
												>
													<div className="flex items-center justify-between">
														<div className="flex items-center space-x-3 flex-1">
															{device.ip_address?.country_code && (
																<img
																	src={getFlagUrl(
																		device.ip_address.country_code,
																	)}
																	alt={
																		device.ip_address.country || "Country flag"
																	}
																	className="w-4 h-3 flex-shrink-0 rounded-sm"
																	onError={(e) => {
																		(
																			e.target as HTMLImageElement
																		).style.display = "none";
																	}}
																/>
															)}
															<span className="font-mono text-sm text-gray-900">
																{getDeviceName(device)}
															</span>
														</div>
														<div className="flex items-center space-x-3 text-sm">
															<span className="text-gray-500">Never</span>
															<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
														</div>
													</div>
												</Link>
											);
										})}
										{neverSeen.length > 10 && (
											<div className="px-4 py-3 bg-gray-50">
												<Link
													href="/devices"
													className="block text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
												>
													View all {neverSeen.length} devices →
												</Link>
											</div>
										)}
									</div>
								</div>
							)}
						</div>
					) : (
						/* All Good Message */
						<div className="bg-green-50 border border-green-200 rounded-lg p-6">
							<div className="flex items-center space-x-3">
								<CheckCircle className="w-6 h-6 text-green-500" />
								<div>
									<h3 className="text-lg font-semibold text-green-900">
										All Systems Operational
									</h3>
									<p className="text-sm text-green-700 mt-1">
										No devices need attention. All updates are successful and
										devices are either online or archived (offline &gt;30 days).
									</p>
								</div>
							</div>
						</div>
					)}
		</div>
	);
};

export default AdminPanel;
