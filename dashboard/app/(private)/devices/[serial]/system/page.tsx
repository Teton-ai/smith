import {
	Button,
	Card,
	CountryFlag,
	InfoRow,
	LabelChip,
	Panel,
	SECTION_THEMES,
} from "@teton/smith-ui";
import {
	Clock,
	GitBranch,
	Globe,
	MapPin,
	RefreshCw,
	Tag,
	Tags,
} from "lucide-react";
import { lazy, Suspense } from "react";
import { Link, useParams } from "react-router";
import { type Device, useGetDeviceInfo } from "@/app/api-client";
import { RelativeTime } from "@/app/components/RelativeTime";
import { DeviceDetailLayout } from "../DeviceDetailLayout";
import {
	SettingValue,
	useDeviceSettings,
	useRefreshDeviceSettings,
} from "../NetworkSettingsPanel";

const LocationMap = lazy(() => import("../LocationMap"));

const MapFallback = () => (
	<div className="h-64 bg-gray-100 rounded-lg flex items-center justify-center">
		Loading map...
	</div>
);

const ServerList = ({ servers }: { servers?: string[] }) =>
	servers && servers.length > 0 ? (
		<span className="flex flex-col items-end font-mono text-xs">
			{servers.map((server) => (
				<span key={server}>{server}</span>
			))}
		</span>
	) : (
		<span className="text-gray-400">None</span>
	);

/** Hostname and time sync as the device reports them (settings report). */
const TimeAndIdentityPanel = ({
	serial,
	device,
}: {
	serial: string;
	device: Device;
}) => {
	const { data: settings, isLoading, refetch } = useDeviceSettings(serial);
	const { refresh, isPending } = useRefreshDeviceSettings(device, refetch);
	const r = settings?.reported;

	return (
		<Panel
			title="Time and identity"
			icon={Clock}
			theme={SECTION_THEMES.blue}
			actions={
				<div className="flex items-center gap-2">
					{r && (
						<span className="text-xs text-gray-500">
							Reported <RelativeTime date={r.reported_at} />
						</span>
					)}
					<Button
						variant="soft"
						tone="gray"
						size="sm"
						loading={isPending}
						onClick={refresh}
						icon={<RefreshCw className="w-3.5 h-3.5" />}
					>
						Refresh
					</Button>
				</div>
			}
		>
			{isLoading || !settings ? (
				<div className="h-4 animate-pulse rounded bg-gray-100" />
			) : !r ? (
				<p className="text-sm text-gray-500">
					Not reported. The device needs a smithd version that reports its
					settings.
				</p>
			) : (
				<div className="space-y-3">
					<InfoRow label="Hostname">
						<SettingValue settings={settings} field="hostname">
							{r.hostname && <span className="font-mono">{r.hostname}</span>}
						</SettingValue>
					</InfoRow>
					<InfoRow label="NTP servers">
						<SettingValue settings={settings} field="ntp_servers">
							<ServerList servers={r.ntp_servers} />
						</SettingValue>
					</InfoRow>
					<InfoRow label="From DHCP">
						<SettingValue settings={settings} field="ntp_link_servers">
							<ServerList servers={r.ntp_link_servers} />
						</SettingValue>
					</InfoRow>
					<InfoRow label="Fallback servers">
						<SettingValue settings={settings} field="ntp_fallback_servers">
							<ServerList servers={r.ntp_fallback_servers} />
						</SettingValue>
					</InfoRow>
					<InfoRow label="Server in use">
						<SettingValue settings={settings} field="ntp_server_in_use">
							{r.ntp_server_in_use && (
								<span className="font-mono text-xs">{r.ntp_server_in_use}</span>
							)}
						</SettingValue>
					</InfoRow>
					<InfoRow label="Clock synchronized">
						<SettingValue settings={settings} field="ntp_synchronized">
							{r.ntp_synchronized !== undefined &&
								(r.ntp_synchronized ? "Yes" : "No")}
						</SettingValue>
					</InfoRow>
				</div>
			)}
		</Panel>
	);
};

const linkClass =
	"font-mono text-sm text-blue-600 hover:text-blue-800 hover:underline cursor-pointer transition-colors";

/** System tab: the device's labels, hardware, OS and release details, next to
 *  where it is. The system card carries no header — the tab already names it —
 *  but location keeps one, since it's a distinct section on the same tab. */
const SystemPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const { data: device } = useGetDeviceInfo(serial);

	return (
		<DeviceDetailLayout serial={serial} device={device} activeTab="system">
			{!device ? (
				<Card className="p-5">
					<div className="py-6 text-gray-500">
						Loading system information...
					</div>
				</Card>
			) : (
				<div className="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
					<div className="space-y-4">
						<Card className="p-5">
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
						</Card>
						<TimeAndIdentityPanel
							key={serial}
							serial={serial}
							device={device}
						/>
					</div>

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
											<span className="font-medium">
												{device.ip_address.isp}
											</span>
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
			)}
		</DeviceDetailLayout>
	);
};

export default SystemPage;
