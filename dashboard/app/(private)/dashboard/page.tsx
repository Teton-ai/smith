import {
	Card,
	CountryFlag,
	type IconComponent,
	ListRow,
	PageContainer,
	SECTION_THEMES,
	SectionCard,
	type SectionTheme,
	StatCard,
	ViewAllFooter,
} from "@teton/smith-ui";
import {
	Activity,
	AlertTriangle,
	CheckCircle,
	ChevronRight,
	Clock,
	Cpu,
	Package,
	ShieldCheck,
	XCircle,
} from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import { Link } from "react-router";
import {
	Area,
	AreaChart,
	CartesianGrid,
	ResponsiveContainer,
	Tooltip,
	XAxis,
	YAxis,
} from "recharts";
import { Modal } from "@/app/components/modal";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import { useConfig } from "@/app/hooks/config";
import {
	type Device,
	useGetDashboard,
	useGetDevices,
	useGetRegistrationCounts,
} from "../../api-client";
import {
	type UnhealthyServiceDevice,
	useUnhealthyServices,
} from "./useUnhealthyServices";

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

// Sort by last_seen descending (most recent first), never seen at the end
const sortByLastSeen = (a: Device, b: Device) => {
	if (!a.last_seen && !b.last_seen) return 0;
	if (!a.last_seen) return 1;
	if (!b.last_seen) return -1;
	return new Date(b.last_seen).getTime() - new Date(a.last_seen).getTime();
};

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

// Device list row: flag + serial on the left, caller-supplied content on the
// right. Built on the shared ListRow + CountryFlag primitives.
const DeviceRow = ({
	to,
	theme,
	device,
	right,
	children,
}: {
	to: string;
	theme: SectionTheme;
	device: Device;
	right: ReactNode;
	children?: ReactNode;
}) => (
	<ListRow to={to} hover={theme.hover}>
		<div className="flex items-center gap-3 min-w-0">
			<CountryFlag
				countryCode={device.ip_address?.country_code}
				country={device.ip_address?.country}
			/>
			<span className="font-mono text-sm text-gray-900 truncate">
				{getDeviceName(device)}
			</span>
			{children}
		</div>
		<div className="flex items-center gap-3 text-sm flex-shrink-0">
			{right}
			<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
		</div>
	</ListRow>
);

// Offline-style section: flag + serial on the left, "time ago" on the right.
const OfflineSection = ({
	icon,
	title,
	theme,
	devices,
	viewAllTo,
}: {
	icon: IconComponent;
	title: string;
	theme: SectionTheme;
	devices: Device[];
	viewAllTo: string;
}) => {
	if (devices.length === 0) return null;
	return (
		<SectionCard
			icon={icon}
			title={title}
			count={devices.length}
			theme={theme}
			footer={
				devices.length > 10 ? (
					<ViewAllFooter to={viewAllTo} count={devices.length} noun="devices" />
				) : undefined
			}
		>
			{devices.slice(0, 10).map((device) => (
				<DeviceRow
					key={device.id}
					to={`/devices/${device.serial_number}`}
					theme={theme}
					device={device}
					right={
						<span className="text-gray-500">
							{device.last_seen ? formatTimeAgo(device.last_seen) : "never"}
						</span>
					}
				/>
			))}
		</SectionCard>
	);
};

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

	const { data: unapprovedDevices = [] } = useGetDevices(
		{ approved: false },
		{ query: { refetchInterval: 5000 } },
	);

	const { data: outdatedDevices = [], isLoading: outdatedLoading } =
		useGetDevices(
			{
				outdated: true,
				outdated_minutes: 30,
				online: true,
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

	const { data: unhealthyServices = [] } = useUnhealthyServices();

	const { data: registrationData } = useGetRegistrationCounts();

	type Granularity = "monthly" | "quarterly" | "yearly";
	const [granularity, setGranularity] = useState<Granularity>("monthly");
	const [chartOpen, setChartOpen] = useState(false);

	const chartData = useMemo(() => {
		if (!registrationData) return [];

		const now = new Date();
		const currentMonth = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
		const currentQuarterStart = Math.floor(now.getMonth() / 3) * 3;
		const currentQuarterKey = `${now.getFullYear()}-${String(currentQuarterStart + 1).padStart(2, "0")}-01`;
		if (granularity === "monthly") {
			return registrationData
				.filter((item) => !item.date.startsWith(currentMonth))
				.map((item) => ({ date: item.date, count: item.count }));
		}

		const buckets = new Map<string, number>();
		for (const item of registrationData) {
			const d = new Date(item.date);
			let key: string;
			if (granularity === "quarterly") {
				const q = Math.floor(d.getMonth() / 3) * 3;
				key = `${d.getFullYear()}-${String(q + 1).padStart(2, "0")}-01`;
			} else {
				key = `${d.getFullYear()}-01-01`;
			}
			buckets.set(key, (buckets.get(key) || 0) + item.count);
		}

		// Remove current incomplete period (except yearly)
		if (granularity === "quarterly") {
			buckets.delete(currentQuarterKey);
		}

		return Array.from(buckets.entries()).map(([date, count]) => ({
			date,
			count,
		}));
	}, [registrationData, granularity]);

	const loading = dashboardQuery.isLoading || outdatedLoading || offlineLoading;

	// Group unhealthy services by device
	const unhealthyByDevice = useMemo(() => {
		const map = new Map<
			string,
			{ serial_number: string; services: UnhealthyServiceDevice[] }
		>();
		for (const entry of unhealthyServices) {
			const existing = map.get(entry.serial_number);
			if (existing) {
				existing.services.push(entry);
			} else {
				map.set(entry.serial_number, {
					serial_number: entry.serial_number,
					services: [entry],
				});
			}
		}
		return Array.from(map.values());
	}, [unhealthyServices]);

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
		unhealthyByDevice.length > 0 ||
		recentlyOffline.length > 0 ||
		offlineWeek.length > 0 ||
		offlineMonth.length > 0 ||
		neverSeen.length > 0;

	return (
		<PageContainer>
			{/* Overview Stats */}
			<div className="grid grid-cols-1 sm:grid-cols-3 gap-4 sm:gap-6">
				{loading ? (
					// Skeleton loading for stats
					[...Array(3)].map((_, index) => (
						<Card key={index} className="p-5">
							<div className="flex items-center gap-4">
								<div className="w-12 h-12 bg-gray-200 rounded-xl animate-pulse" />
								<div>
									<div className="h-4 bg-gray-200 rounded w-16 animate-pulse mb-2" />
									<div className="h-8 bg-gray-200 rounded w-12 animate-pulse" />
								</div>
							</div>
						</Card>
					))
				) : (
					<>
						<Link
							to="/devices?approved=false"
							className={`group rounded-xl border p-5 shadow-sm transition-all hover:shadow-md ${
								unapprovedDevices.length > 0
									? "bg-orange-50 border-orange-200 hover:border-orange-300"
									: "bg-white border-gray-200/80 hover:border-gray-300"
							}`}
						>
							<div className="flex items-center justify-between">
								<StatCard
									icon={ShieldCheck}
									label="Pending Approval"
									value={unapprovedDevices.length}
									tone={unapprovedDevices.length > 0 ? "orange" : "neutral"}
								/>
								<ChevronRight
									className={`w-5 h-5 transition-transform group-hover:translate-x-0.5 ${
										unapprovedDevices.length > 0
											? "text-orange-400"
											: "text-gray-300"
									}`}
								/>
							</div>
						</Link>

						<Card className="p-5">
							<StatCard
								icon={CheckCircle}
								label="Online"
								value={dashboardQuery.data?.online_count || 0}
								tone="green"
							/>
						</Card>

						<button
							type="button"
							onClick={() => setChartOpen(true)}
							className="group bg-white rounded-xl border border-gray-200/80 p-5 shadow-sm cursor-pointer hover:shadow-md hover:border-gray-300 transition-all text-left relative overflow-hidden"
						>
							{chartData.length > 1 && (
								<div className="absolute inset-0 top-1/3">
									<ResponsiveContainer width="100%" height="100%">
										<AreaChart data={chartData}>
											<defs>
												<linearGradient
													id="sparkline"
													x1="0"
													y1="0"
													x2="0"
													y2="1"
												>
													<stop
														offset="5%"
														stopColor="#3b82f6"
														stopOpacity={0.15}
													/>
													<stop
														offset="95%"
														stopColor="#3b82f6"
														stopOpacity={0.03}
													/>
												</linearGradient>
											</defs>
											<Area
												type="monotone"
												dataKey="count"
												stroke="#3b82f6"
												strokeWidth={1.5}
												strokeOpacity={0.3}
												fill="url(#sparkline)"
											/>
										</AreaChart>
									</ResponsiveContainer>
								</div>
							)}
							<div className="relative z-10">
								<StatCard
									icon={Cpu}
									label="Total Devices"
									value={dashboardQuery.data?.total_count || 0}
									tone="blue"
								/>
							</div>
							{chartData.length > 1 && (
								<span className="absolute bottom-2 right-3 text-[10px] text-gray-400 opacity-0 group-hover:opacity-100 transition-opacity z-10">
									Click to view new devices over time
								</span>
							)}
						</button>
					</>
				)}
			</div>

			{/* Device Status Sections */}
			{loading ? (
				<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
					{[...Array(4)].map((_, sectionIndex) => (
						<div
							key={sectionIndex}
							className="bg-white rounded-xl border border-gray-200/80 shadow-sm"
						>
							<div className="px-4 py-3 border-b border-gray-100">
								<div className="h-5 bg-gray-200 rounded w-32 animate-pulse" />
							</div>
							<div className="divide-y divide-gray-100">
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
						<SectionCard
							icon={Package}
							title="Pending Update"
							count={stuckUpdates.length}
							theme={SECTION_THEMES.purple}
							footer={
								stuckUpdates.length > 10 ? (
									<ViewAllFooter
										to="/devices?outdated=true"
										count={stuckUpdates.length}
										noun="devices"
									/>
								) : undefined
							}
						>
							{stuckUpdates.slice(0, 10).map((device) => {
								const isOnline = device.last_seen
									? (Date.now() - new Date(device.last_seen).getTime()) /
											(1000 * 60) <=
										3
									: false;
								return (
									<DeviceRow
										key={device.id}
										to={`/devices/${device.serial_number}`}
										theme={SECTION_THEMES.purple}
										device={device}
										right={
											<>
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
											</>
										}
									>
										{device.network?.network_score && (
											<NetworkQualityIndicator
												isOnline={isOnline}
												networkScore={device.network.network_score}
											/>
										)}
									</DeviceRow>
								);
							})}
						</SectionCard>
					)}

					{/* Unhealthy Services Section */}
					{unhealthyByDevice.length > 0 && (
						<SectionCard
							icon={Activity}
							title="Service Not Running"
							count={unhealthyByDevice.length}
							theme={SECTION_THEMES.rose}
							footer={
								unhealthyByDevice.length > 10 ? (
									<ViewAllFooter
										to="/devices?service_not_running=true&online=online"
										count={unhealthyByDevice.length}
										noun="devices"
									/>
								) : undefined
							}
						>
							{unhealthyByDevice.slice(0, 10).map((device) => (
								<ListRow
									key={device.serial_number}
									to={`/devices/${device.serial_number}`}
									hover={SECTION_THEMES.rose.hover}
								>
									<span className="font-mono text-sm text-gray-900 truncate">
										{device.serial_number}
									</span>
									<div className="flex items-center gap-3 text-sm flex-shrink-0">
										<span className="text-rose-600 font-mono">
											{device.services.map((s) => s.service_name).join(", ")}
										</span>
										<ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
									</div>
								</ListRow>
							))}
						</SectionCard>
					)}

					<OfflineSection
						icon={Clock}
						title="Recently Offline"
						theme={SECTION_THEMES.yellow}
						devices={recentlyOffline}
						viewAllTo="/devices"
					/>

					<OfflineSection
						icon={AlertTriangle}
						title="Offline This Week"
						theme={SECTION_THEMES.orange}
						devices={offlineWeek}
						viewAllTo="/devices"
					/>

					<OfflineSection
						icon={XCircle}
						title="Long-term Offline"
						theme={SECTION_THEMES.red}
						devices={offlineMonth}
						viewAllTo="/devices"
					/>

					<OfflineSection
						icon={AlertTriangle}
						title="Never Connected"
						theme={SECTION_THEMES.gray}
						devices={neverSeen}
						viewAllTo="/devices"
					/>
				</div>
			) : (
				/* All Good Message */
				<Card className="p-10">
					<div className="flex flex-col items-center text-center">
						<div className="flex h-14 w-14 items-center justify-center rounded-full bg-green-100 mb-4">
							<CheckCircle className="w-8 h-8 text-green-600" />
						</div>
						<h3 className="text-lg font-semibold text-gray-900">
							All Systems Operational
						</h3>
						<p className="text-sm text-gray-500 mt-1 max-w-md">
							No devices need attention. All updates are successful and devices
							are either online or archived (offline &gt;30 days).
						</p>
					</div>
				</Card>
			)}
			{/* Registration Chart Modal */}
			<Modal
				open={chartOpen}
				onClose={() => setChartOpen(false)}
				title="New Devices"
				subtitle={`New devices added per ${granularity === "quarterly" ? "quarter" : granularity === "yearly" ? "year" : "month"}`}
				width="w-[700px]"
				headerRight={
					<div className="flex rounded-md border border-gray-200 text-xs">
						{(["monthly", "quarterly", "yearly"] as const).map((g) => (
							<button
								key={g}
								type="button"
								onClick={() => setGranularity(g)}
								className={`px-2.5 py-1 capitalize cursor-pointer transition-colors ${
									granularity === g
										? "bg-blue-50 text-blue-700 font-medium"
										: "text-gray-500 hover:bg-gray-50"
								} ${g !== "monthly" ? "border-l border-gray-200" : ""}`}
							>
								{g}
							</button>
						))}
					</div>
				}
			>
				{chartData.length > 1 ? (
					<div className="-mx-4">
						<ResponsiveContainer width="100%" height={300}>
							<AreaChart data={chartData} margin={{ left: -20, right: 10 }}>
								<defs>
									<linearGradient id="colorModal" x1="0" y1="0" x2="0" y2="1">
										<stop offset="5%" stopColor="#3b82f6" stopOpacity={0.2} />
										<stop offset="95%" stopColor="#3b82f6" stopOpacity={0} />
									</linearGradient>
								</defs>
								<CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
								<XAxis
									dataKey="date"
									tick={{ fontSize: 11, fill: "#6b7280" }}
									tickLine={false}
									axisLine={{ stroke: "#e5e7eb" }}
									tickFormatter={(value: string) => {
										const d = new Date(value);
										if (granularity === "yearly")
											return d.getFullYear().toString();
										if (granularity === "quarterly") {
											const q = Math.floor(d.getMonth() / 3) + 1;
											return `Q${q} ${d.getFullYear().toString().slice(2)}`;
										}
										return `${d.toLocaleString("default", { month: "short" })} ${d.getFullYear().toString().slice(2)}`;
									}}
									interval="preserveStartEnd"
									minTickGap={50}
								/>
								<YAxis
									tick={{ fontSize: 11, fill: "#6b7280" }}
									tickLine={false}
									axisLine={false}
									allowDecimals={false}
								/>
								<Tooltip
									contentStyle={{
										borderRadius: "8px",
										border: "1px solid #e5e7eb",
										fontSize: "12px",
										color: "#111827",
									}}
									labelStyle={{ color: "#374151", fontWeight: 500 }}
									itemStyle={{ color: "#1d4ed8" }}
									labelFormatter={(label: string) => {
										const d = new Date(label);
										if (granularity === "yearly")
											return d.getFullYear().toString();
										if (granularity === "quarterly") {
											const q = Math.floor(d.getMonth() / 3) + 1;
											return `Q${q} ${d.getFullYear()}`;
										}
										return `${d.toLocaleString("default", { month: "long" })} ${d.getFullYear()}`;
									}}
									formatter={(value: number) => [value, "New devices"]}
								/>
								<Area
									type="monotone"
									dataKey="count"
									stroke="#3b82f6"
									strokeWidth={2}
									fill="url(#colorModal)"
								/>
							</AreaChart>
						</ResponsiveContainer>
					</div>
				) : (
					<p className="text-sm text-gray-500 text-center py-8">
						Not enough data to display chart.
					</p>
				)}
			</Modal>
		</PageContainer>
	);
};

export default AdminPanel;
