import { Badge, InfoRow, Panel, SECTION_THEMES } from "@teton/smith-ui";
import { Router, Signal, Smartphone, Wifi, WifiOff } from "lucide-react";
import { useParams } from "react-router";
import {
	type Device,
	type NetworkItem,
	useGetDeviceInfo,
} from "@/app/api-client";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import { RelativeTime } from "@/app/components/RelativeTime";
import { DeviceDetailLayout } from "../DeviceDetailLayout";
import { DeviceReachability } from "../uptime";
import WifiPanel from "../WifiPanel";

/** The link the device is actually using, in the order it would prefer them. */
const primaryLink = (device: Device) => {
	const connected =
		device.system_info?.connection_statuses?.filter(
			(c) => c.connection_state === "connected",
		) ?? [];

	if (device.modem_id && device.modem) {
		const { network_provider, imei } = device.modem;
		return {
			icon: Signal,
			iconClass: "text-blue-600",
			label: "Cellular",
			detail: [network_provider, imei && `IMEI ${imei}`]
				.filter(Boolean)
				.join(" · "),
		};
	}

	const wifi = connected.find((c) => c.device_type === "wifi");
	if (wifi) {
		return {
			icon: Wifi,
			iconClass: "text-green-600",
			label: "WiFi",
			detail: wifi.connection_name ?? "",
		};
	}

	const ethernet = connected.filter((c) => c.device_type === "ethernet");
	if (ethernet.length > 0) {
		return {
			icon: Router,
			iconClass: "text-orange-600",
			label: "Ethernet",
			detail: ethernet.map((c) => c.device_name).join(", "),
		};
	}

	return connected.length > 0
		? {
				icon: Smartphone,
				iconClass: "text-gray-600",
				label: "Other",
				detail: "",
			}
		: null;
};

const qualityLabel = (score: number) =>
	score >= 4 ? "Excellent" : score === 3 ? "Good" : "Poor";

/**
 * Which link the device is on and how well it performs. This used to be two
 * tooltips on the device header, where the numbers couldn't be read or copied.
 */
const LinkSummary = ({ device }: { device: Device }) => {
	const link = primaryLink(device);
	const net = device.network;
	const online =
		device.last_seen != null &&
		Date.now() - new Date(device.last_seen).getTime() <= 3 * 60_000;

	if (!link && !net) {
		return (
			<p className="text-sm text-gray-500">No connection details reported</p>
		);
	}

	return (
		<div className="space-y-2 text-sm">
			{link && (
				<InfoRow
					label="Primary link"
					icon={link.icon}
					iconClassName={link.iconClass}
				>
					{link.label}
					{link.detail && (
						<span className="text-gray-500"> · {link.detail}</span>
					)}
				</InfoRow>
			)}

			{net?.network_score != null && (
				<InfoRow label="Quality">
					<span className="inline-flex items-center gap-1.5">
						<NetworkQualityIndicator
							isOnline={online}
							networkScore={net.network_score}
						/>
						{qualityLabel(net.network_score)} ({net.network_score}/5)
					</span>
				</InfoRow>
			)}

			{(net?.download_speed_mbps != null || net?.upload_speed_mbps != null) && (
				<InfoRow label="Speed">
					<span className="font-mono tabular-nums">
						{net.download_speed_mbps != null &&
							`↓ ${net.download_speed_mbps.toFixed(1)}`}
						{net.download_speed_mbps != null &&
							net.upload_speed_mbps != null &&
							" / "}
						{net.upload_speed_mbps != null &&
							`↑ ${net.upload_speed_mbps.toFixed(1)}`}
					</span>{" "}
					<span className="text-gray-500">Mbps</span>
				</InfoRow>
			)}

			{net?.updated_at && (
				<InfoRow label="Last tested">
					<span className="text-gray-500">
						<RelativeTime date={net.updated_at} />
					</span>
				</InfoRow>
			)}
		</div>
	);
};

const ConnectionCard = ({
	name,
	iface,
	deviceType,
	connected,
}: {
	name: string;
	iface: NetworkItem;
	deviceType: string;
	connected: boolean;
	virtual?: boolean;
}) => {
	const primaryIP = iface.ips[0];
	const Icon =
		deviceType === "wifi"
			? connected
				? Wifi
				: WifiOff
			: deviceType === "ethernet"
				? Router
				: Smartphone;
	const iconClass = !connected
		? "text-gray-400"
		: deviceType === "wifi"
			? "text-green-600"
			: deviceType === "ethernet"
				? "text-blue-600"
				: "text-gray-600";

	return (
		<div
			className={`p-3 border rounded-lg ${
				connected
					? "border-green-200 bg-green-50"
					: "border-gray-200 bg-gray-50"
			}`}
		>
			<div className="flex items-center justify-between mb-2">
				<div className="flex items-center space-x-2">
					<Icon className={`w-4 h-4 ${iconClass}`} />
					<span className="font-mono text-sm font-medium text-gray-900">
						{name}
					</span>
				</div>
				<Badge variant={connected ? "green" : "gray"} pill>
					{connected ? "Connected" : "Disconnected"}
				</Badge>
			</div>

			<div className="space-y-2 text-sm">
				{primaryIP && (
					<div className="flex justify-between">
						<span className="text-gray-600">Primary IP</span>
						<span className="font-mono text-gray-900">{primaryIP}</span>
					</div>
				)}
				<div className="flex justify-between">
					<span className="text-gray-600">MAC Address</span>
					<span className="font-mono text-gray-900">{iface.mac_address}</span>
				</div>
				{iface.ips.length > 1 && (
					<div className="flex justify-between">
						<span className="text-gray-600">Additional IPs</span>
						<div className="text-right">
							{iface.ips.slice(1).map((ip) => (
								<div key={ip} className="font-mono text-gray-900">
									{ip}
								</div>
							))}
						</div>
					</div>
				)}
			</div>
		</div>
	);
};

/**
 * NM device types that aren't a link to anywhere: docker/libvirt bridges,
 * loopback, VPN taps. They're reported like any other interface and show as
 * "connected", so without this they sit at the top of the list looking as
 * relevant as eth0. Unknown types stay in the main list — better to show an
 * interface we can't classify than to bury a real one.
 */
const VIRTUAL_TYPES = new Set([
	"bridge",
	"loopback",
	"tun",
	"tap",
	"veth",
	"dummy",
	"wireguard",
	"ovs-interface",
	"ovs-bridge",
	"ovs-port",
]);

const NetworkConnections = ({ device }: { device: Device }) => {
	const interfaces = device.system_info?.network?.interfaces;

	if (!interfaces) {
		return (
			<p className="text-gray-500 text-sm">
				No network interface information available
			</p>
		);
	}

	const entries = Object.entries(interfaces).map(([name, iface]) => {
		const status = device.system_info?.connection_statuses?.find(
			(conn) => conn.device_name === name,
		);
		const deviceType = status?.device_type || "unknown";
		return {
			name,
			iface,
			deviceType,
			connected: status?.connection_state === "connected",
			virtual: VIRTUAL_TYPES.has(deviceType),
		};
	});

	if (entries.length === 0) {
		return (
			<div className="flex items-center text-gray-500 text-sm">
				<WifiOff className="w-4 h-4 mr-2" />
				No network interfaces found
			</div>
		);
	}

	// Fall back to every connected interface rather than showing nothing on a
	// device whose only live interfaces are virtual.
	let primary = entries.filter((e) => e.connected && !e.virtual);
	if (primary.length === 0) primary = entries.filter((e) => e.connected);
	const rest = entries.filter((e) => !primary.includes(e));

	return (
		<div className="space-y-3">
			{primary.map((entry) => (
				<ConnectionCard key={entry.name} {...entry} />
			))}

			{rest.length > 0 && (
				<details className="mt-3">
					<summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800">
						Show other interfaces ({rest.length})
					</summary>
					<div className="mt-2 space-y-2">
						{rest.map((entry) => (
							<ConnectionCard key={entry.name} {...entry} />
						))}
					</div>
				</details>
			)}
		</div>
	);
};

/** Network tab: whether we heard from the device at all, the interfaces it
 *  reports, and its WiFi intent, configured profiles and scan results. */
const NetworkPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const { data: device } = useGetDeviceInfo(serial);

	// One layout in both states: Reachability and the WiFi panel only need the
	// serial, so a missing device downgrades the Connections panel rather than
	// replacing the page. Connections is a narrow, fixed-size list; the other
	// two stack in the wider column so the room under Reachability is used
	// instead of pushing WiFi below the fold.
	return (
		<DeviceDetailLayout serial={serial} device={device} activeTab="network">
			<div className="grid grid-cols-1 lg:grid-cols-3 gap-4 items-start">
				<Panel
					title="Network Connections"
					icon={Wifi}
					theme={SECTION_THEMES.green}
				>
					{device ? (
						<>
							<LinkSummary device={device} />
							<hr className="my-3 border-gray-100" />
							<NetworkConnections device={device} />
						</>
					) : (
						<div className="space-y-2">
							{[1, 2, 3].map((i) => (
								<div
									key={i}
									className="h-4 animate-pulse rounded bg-gray-100"
								/>
							))}
						</div>
					)}
				</Panel>

				<div className="lg:col-span-2 space-y-4">
					{/* Whether we heard from the device, not what its interfaces report */}
					<DeviceReachability key={serial} serial={serial} />
					{/* Keyed by serial so filter/reveal state resets when navigating
					    between devices. */}
					{device && <WifiPanel key={serial} serial={serial} device={device} />}
				</div>
			</div>
		</DeviceDetailLayout>
	);
};

export default NetworkPage;
