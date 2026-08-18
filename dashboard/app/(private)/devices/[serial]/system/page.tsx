import {
	Card,
	CountryFlag,
	InfoRow,
	LabelChip,
	Panel,
	SECTION_THEMES,
} from "@teton/smith-ui";
import { GitBranch, Globe, MapPin, Tag, Tags } from "lucide-react";
import { lazy, Suspense } from "react";
import { Link, useParams } from "react-router";
import { useGetDeviceInfo } from "@/app/api-client";
import { DeviceDetailLayout } from "../DeviceDetailLayout";
import { ReleaseIntentCard } from "./ReleaseIntentCard";

const LocationMap = lazy(() => import("../LocationMap"));

const MapFallback = () => (
	<div className="h-64 bg-gray-100 rounded-lg flex items-center justify-center">
		Loading map...
	</div>
);

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

					<ReleaseIntentCard
						deviceId={device.id}
						currentReleaseId={device.release_id}
					/>
				</div>
			)}
		</DeviceDetailLayout>
	);
};

export default SystemPage;
