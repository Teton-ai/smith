import {
	Background,
	Controls,
	type Edge,
	Handle,
	type Node,
	type NodeProps,
	Position,
	ReactFlow,
	type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Card, PageContainer, SearchInput } from "@teton/smith-ui";
import { Cpu, Network, Router, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
	type Lan,
	type LanDevice,
	type LanSummary,
	useGetDevices,
	useGetLan,
	useGetLans,
	useGetLansForDevice,
} from "@/app/api-client";

const DEVICE_W = 170;
const DEVICE_H = 44;
const ROUTER_W = 220;
const ROUTER_H = 64;
// Devices sit on ellipses around their gateway. The ellipse is wider than tall
// because device cards are wide, so neighbours at the top/bottom (side by side)
// and at the left/right (stacked) get the same angular spacing without overlap.
const RING_RY = 110;
const RING_STEP = 60;
const RING_ASPECT = 3.3;
const SLOT = DEVICE_W + 10;
const LAN_GAP = 120;
// Collapsed LANs in the overview are packed into rows about this wide.
const OVERVIEW_ROW_W = 6 * (ROUTER_W + 40);
const OVERVIEW_GAP = 40;

type DeviceData = { device: LanDevice; focused: boolean };
type RouterData = {
	network: string;
	gatewayIp: string;
	publicIp?: string;
	publicIpName?: string;
	// Only in the overview, where devices are fetched on expand.
	counts?: { devices: number; online: number };
	expanded?: boolean;
};

// Edges run center to center, so each node gets one invisible centered handle.
const centerHandle = {
	top: "50%",
	left: "50%",
	transform: "translate(-50%, -50%)",
	opacity: 0,
	pointerEvents: "none" as const,
};

const RouterNode = ({ data }: NodeProps<Node<RouterData>>) => {
	const { network, gatewayIp, publicIp, publicIpName, counts, expanded } = data;
	return (
		<div
			className={`flex h-full w-full items-center gap-3 rounded-xl border-2 bg-blue-50 px-3 shadow-sm ${
				counts ? "cursor-pointer hover:border-blue-500" : ""
			} ${expanded ? "border-blue-500 ring-2 ring-blue-200" : "border-blue-300"}`}
		>
			<Handle
				type="source"
				position={Position.Top}
				style={centerHandle}
				isConnectable={false}
			/>
			<Router className="h-6 w-6 flex-shrink-0 text-blue-500" />
			<div className="min-w-0 flex-1 text-xs">
				<div className="flex items-center justify-between gap-2">
					<span className="font-mono font-semibold text-gray-900">
						{network}
					</span>
					{counts && (
						<span className="flex-shrink-0 rounded-full bg-white px-1.5 text-[10px] text-gray-600">
							<span className="text-green-600">{counts.online}</span>/
							{counts.devices}
						</span>
					)}
				</div>
				<div className="truncate font-mono text-gray-500">gw {gatewayIp}</div>
				{publicIp && (
					<div className="truncate text-gray-500">
						{publicIpName ? `${publicIpName} · ${publicIp}` : publicIp}
					</div>
				)}
			</div>
		</div>
	);
};

const DeviceNode = ({ data }: NodeProps<Node<DeviceData>>) => {
	const { device, focused } = data;
	return (
		<div
			className={`flex h-full w-full cursor-pointer items-center gap-2 rounded-md border bg-white px-2.5 shadow-sm hover:border-blue-400 ${
				focused ? "border-blue-500 ring-2 ring-blue-200" : "border-gray-200"
			}`}
		>
			<Handle
				type="target"
				position={Position.Top}
				style={centerHandle}
				isConnectable={false}
			/>
			<span
				className={`h-2 w-2 flex-shrink-0 rounded-full ${
					device.online ? "bg-green-500" : "bg-gray-300"
				}`}
			/>
			<div className="min-w-0">
				<div className="truncate font-mono text-xs text-gray-900">
					{device.serial_number}
				</div>
				<div className="truncate font-mono text-[10px] text-gray-500">
					{device.address} · {device.interface}
				</div>
			</div>
		</div>
	);
};

const nodeTypes = { router: RouterNode, device: DeviceNode };

/** Fills rings outward; returns each device's offset from the gateway and the
 *  outermost ring's radii. */
const ringLayout = (count: number) => {
	const points: { x: number; y: number }[] = [];
	let ring = 0;
	let rx = RING_RY * RING_ASPECT;
	let ry = RING_RY;
	while (points.length < count) {
		ry = RING_RY + ring * RING_STEP;
		rx = ry * RING_ASPECT;
		const capacity = Math.max(1, Math.floor((2 * Math.PI * rx) / SLOT / 1.2));
		const n = Math.min(capacity, count - points.length);
		// Start at the bottom so the first device (the focused one) sits there.
		for (let i = 0; i < n; i++) {
			const angle = Math.PI / 2 + (2 * Math.PI * i) / n;
			points.push({ x: rx * Math.cos(angle), y: ry * Math.sin(angle) });
		}
		ring++;
	}
	return { points, rx, ry };
};

const routerNode = (
	key: string,
	x: number,
	y: number,
	data: RouterData,
): Node => ({
	id: `lan:${key}`,
	type: "router",
	position: { x: x - ROUTER_W / 2, y: y - ROUTER_H / 2 },
	width: ROUTER_W,
	height: ROUTER_H,
	style: { width: ROUTER_W, height: ROUTER_H },
	data,
	draggable: false,
	selectable: false,
});

/** Places `devices` on rings around the router node `routerId` centered at
 *  (cx, cy). Points come from `ringLayout(devices.length)`. */
const placeDevices = (
	nodes: Node[],
	edges: Edge[],
	routerId: string,
	devices: LanDevice[],
	points: { x: number; y: number }[],
	cx: number,
	cy: number,
	focusedId?: number,
) => {
	devices.forEach((device, i) => {
		const id = `${routerId}:${device.id}`;
		const focused = device.id === focusedId;
		nodes.push({
			id,
			type: "device",
			position: {
				x: cx + points[i].x - DEVICE_W / 2,
				y: cy + points[i].y - DEVICE_H / 2,
			},
			width: DEVICE_W,
			height: DEVICE_H,
			style: { width: DEVICE_W, height: DEVICE_H },
			data: { device, focused },
			draggable: false,
		});
		edges.push({
			id: `edge:${id}`,
			source: routerId,
			target: id,
			type: "straight",
			animated: focused,
			style: focused
				? { stroke: "#3b82f6", strokeWidth: 2 }
				: { stroke: "#cbd5e1" },
		});
	});
};

const publicIpOf = (lan: Lan) => {
	const device = lan.devices.find((d) => d.public_ip);
	return { publicIp: device?.public_ip, publicIpName: device?.public_ip_name };
};

/** One device's LANs side by side, with the device first on each ring. */
const buildFocusGraph = (lans: Lan[], focusedId: number) => {
	const nodes: Node[] = [];
	const edges: Edge[] = [];
	let cx = 0;

	for (const lan of lans) {
		const devices = [...lan.devices].sort(
			(a, b) => Number(b.id === focusedId) - Number(a.id === focusedId),
		);
		const { points, rx } = ringLayout(devices.length);
		cx += rx + DEVICE_W / 2;
		const router = routerNode(lan.key, cx, 0, {
			network: lan.network,
			gatewayIp: lan.gateway_ip,
			...publicIpOf(lan),
		});
		nodes.push(router);
		placeDevices(nodes, edges, router.id, devices, points, cx, 0, focusedId);
		cx += rx + DEVICE_W / 2 + LAN_GAP;
	}

	return { nodes, edges };
};

/** Every LAN packed into rows; the expanded one (once its devices are loaded)
 *  takes the space of its rings. */
const buildOverviewGraph = (
	lans: LanSummary[],
	expandedKey: string | null,
	expanded: Lan | undefined,
) => {
	const nodes: Node[] = [];
	const edges: Edge[] = [];
	const ring =
		expanded && expanded.key === expandedKey
			? ringLayout(expanded.devices.length)
			: null;
	const rowW = Math.max(OVERVIEW_ROW_W, ring ? 2 * ring.rx + DEVICE_W : 0);

	type Cell = { lan: LanSummary; w: number; h: number };
	const rows: Cell[][] = [[]];
	let x = 0;
	for (const lan of lans) {
		const isExpanded = ring && lan.key === expandedKey;
		const w = isExpanded ? 2 * ring.rx + DEVICE_W : ROUTER_W;
		const h = isExpanded ? 2 * ring.ry + DEVICE_H : ROUTER_H;
		const row = rows[rows.length - 1];
		if (row.length > 0 && x + w > rowW) {
			rows.push([]);
			x = 0;
		}
		rows[rows.length - 1].push({ lan, w, h });
		x += w + OVERVIEW_GAP;
	}

	let y = 0;
	for (const row of rows) {
		const rowH = Math.max(...row.map((c) => c.h));
		let cx = 0;
		for (const { lan, w } of row) {
			const isExpanded = lan.key === expandedKey;
			const router = routerNode(lan.key, cx + w / 2, y + rowH / 2, {
				network: lan.network,
				gatewayIp: lan.gateway_ip,
				publicIp: lan.public_ip,
				publicIpName: lan.public_ip_name,
				counts: { devices: lan.device_count, online: lan.online_count },
				expanded: isExpanded,
			});
			nodes.push(router);
			if (isExpanded && ring && expanded) {
				placeDevices(
					nodes,
					edges,
					router.id,
					expanded.devices,
					ring.points,
					cx + w / 2,
					y + rowH / 2,
				);
			}
			cx += w + OVERVIEW_GAP;
		}
		y += rowH + OVERVIEW_GAP;
	}

	return { nodes, edges };
};

/** Splits a LAN key (`gateway_mac|network`) back into its query params. */
const lanParams = (key: string) => {
	const [gateway_mac = "", network = ""] = key.split("|");
	return { gateway_mac, network };
};

const useDebounced = <T,>(value: T, ms: number): T => {
	const [debounced, setDebounced] = useState(value);
	useEffect(() => {
		const t = setTimeout(() => setDebounced(value), ms);
		return () => clearTimeout(t);
	}, [value, ms]);
	return debounced;
};

const DevicePicker = ({ onPick }: { onPick: (serial: string) => void }) => {
	const [search, setSearch] = useState("");
	const term = useDebounced(search.trim(), 200);
	const { data: devices = [], isFetching } = useGetDevices(
		{ search: term || undefined, limit: 8 },
		{ query: { enabled: term.length > 0 } },
	);

	return (
		<div className="relative w-full sm:w-80">
			<SearchInput
				value={search}
				onChange={setSearch}
				placeholder="Find a device by serial..."
				className="w-full"
				slashToFocus
			/>
			{term && (
				<Card className="absolute z-10 mt-1 w-full overflow-hidden">
					{devices.length === 0 ? (
						<p className="px-3 py-2 text-sm text-gray-500">
							{isFetching ? "Searching..." : "No devices found"}
						</p>
					) : (
						<ul className="divide-y divide-gray-100">
							{devices.map((d) => (
								<li key={d.id}>
									<button
										type="button"
										onClick={() => {
											onPick(d.serial_number);
											setSearch("");
										}}
										className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-gray-50"
									>
										<span
											className={`h-2 w-2 flex-shrink-0 rounded-full ${
												d.online ? "bg-green-500" : "bg-gray-300"
											}`}
										/>
										<span className="truncate font-mono text-gray-900">
											{d.serial_number}
										</span>
									</button>
								</li>
							))}
						</ul>
					)}
				</Card>
			)}
		</div>
	);
};

const DeviceRow = ({
	device,
	onPick,
}: {
	device: LanDevice;
	onPick: (serial: string) => void;
}) => (
	<li>
		<button
			type="button"
			onClick={() => onPick(device.serial_number)}
			className="flex w-full items-center justify-between gap-2 rounded px-2 py-1 text-sm hover:bg-gray-50"
		>
			<span className="flex min-w-0 items-center gap-2">
				<span
					className={`h-2 w-2 flex-shrink-0 rounded-full ${
						device.online ? "bg-green-500" : "bg-gray-300"
					}`}
				/>
				<span className="truncate font-mono text-gray-900">
					{device.serial_number}
				</span>
			</span>
			<span className="font-mono text-xs text-gray-600">{device.address}</span>
		</button>
	</li>
);

const LanPanel = ({
	lan,
	onPick,
	onClose,
}: {
	lan: Lan;
	onPick: (serial: string) => void;
	onClose: () => void;
}) => (
	<Card className="w-full lg:w-80 flex-shrink-0 overflow-hidden">
		<div className="flex items-center justify-between border-b border-gray-100 px-4 py-3">
			<div className="min-w-0 text-xs text-gray-500">
				<div className="flex items-center gap-2 font-mono text-sm font-semibold text-gray-900">
					<Router className="h-4 w-4 text-gray-400" />
					{lan.network}
				</div>
				<div className="font-mono">
					gw {lan.gateway_ip} · {lan.gateway_mac}
				</div>
			</div>
			<button
				type="button"
				onClick={onClose}
				className="text-gray-400 hover:text-gray-600"
				aria-label="Collapse LAN"
			>
				<X className="h-4 w-4" />
			</button>
		</div>
		<ul className="max-h-[600px] space-y-1 overflow-y-auto px-4 py-3">
			{lan.devices.map((device) => (
				<DeviceRow
					key={`${device.id}:${device.interface}`}
					device={device}
					onPick={onPick}
				/>
			))}
		</ul>
	</Card>
);

const PeerPanel = ({
	lans,
	device,
	onPick,
	onClose,
}: {
	lans: Lan[];
	device: LanDevice;
	onPick: (serial: string) => void;
	onClose: () => void;
}) => (
	<Card className="w-full lg:w-80 flex-shrink-0 overflow-hidden">
		<div className="flex items-center justify-between border-b border-gray-100 px-4 py-3">
			<Link
				to={`/devices/${device.serial_number}/network`}
				className="flex items-center gap-2 truncate font-mono text-sm font-semibold text-gray-900 hover:text-blue-600"
			>
				<Cpu className="h-4 w-4 text-gray-400" />
				{device.serial_number}
			</Link>
			<button
				type="button"
				onClick={onClose}
				className="text-gray-400 hover:text-gray-600"
				aria-label="Clear selection"
			>
				<X className="h-4 w-4" />
			</button>
		</div>
		<div className="max-h-[600px] divide-y divide-gray-100 overflow-y-auto">
			{lans.map((lan) => {
				const self = lan.devices.find((d) => d.id === device.id);
				const peers = lan.devices.filter((d) => d.id !== device.id);
				return (
					<div key={lan.key} className="px-4 py-3 space-y-2">
						<div className="text-xs text-gray-500">
							<span className="font-mono text-gray-800">{lan.network}</span> via{" "}
							{self?.interface} ({self?.address})
							<div className="font-mono">
								gw {lan.gateway_ip} · {lan.gateway_mac}
							</div>
						</div>
						{peers.length === 0 ? (
							<p className="text-sm text-gray-500">
								No other devices on this LAN
							</p>
						) : (
							<ul className="space-y-1">
								{peers.map((peer) => (
									<DeviceRow key={peer.id} device={peer} onPick={onPick} />
								))}
							</ul>
						)}
					</div>
				);
			})}
		</div>
	</Card>
);

/** Without a device: every LAN in the fleet as a gateway node; clicking one
 *  expands it to show its devices (fetched only then, since LANs can be big).
 *  With a device: that device's LANs with its neighbours around each gateway;
 *  clicking a neighbour moves the focus to it. */
const NetworkMapPage = () => {
	const [searchParams, setSearchParams] = useSearchParams();
	const [flow, setFlow] = useState<ReactFlowInstance | null>(null);
	const serial = searchParams.get("device") ?? "";
	const expandedKey = serial ? null : searchParams.get("lan");

	const {
		data: lans = [],
		isLoading: lansLoading,
		isError: lansError,
	} = useGetLansForDevice(serial, {
		query: { enabled: serial.length > 0, select: (data) => data.lans },
	});
	const {
		data: allLans = [],
		isLoading: allLansLoading,
		isError: allLansError,
	} = useGetLans({
		query: { enabled: !serial, select: (data) => data.lans },
	});
	const { data: expanded } = useGetLan(lanParams(expandedKey ?? ""), {
		query: { enabled: !!expandedKey },
	});
	const isLoading = serial ? lansLoading : allLansLoading;
	const isError = serial ? lansError : allLansError;

	const focused = useMemo(
		() =>
			lans.flatMap((l) => l.devices).find((d) => d.serial_number === serial) ??
			null,
		[lans, serial],
	);

	const { nodes, edges } = useMemo(() => {
		if (serial)
			return focused
				? buildFocusGraph(lans, focused.id)
				: { nodes: [], edges: [] };
		return buildOverviewGraph(allLans, expandedKey, expanded);
	}, [serial, lans, focused, allLans, expandedKey, expanded]);

	const select = (next: string | null) => {
		const params = new URLSearchParams(searchParams);
		params.delete("lan");
		if (next) params.set("device", next);
		else params.delete("device");
		setSearchParams(params);
	};

	const expand = (key: string | null) => {
		const params = new URLSearchParams(searchParams);
		if (key) params.set("lan", key);
		else params.delete("lan");
		setSearchParams(params);
	};

	const expandedLoaded = !!expanded && expanded.key === expandedKey;

	// Zoom onto the expanded LAN once its devices are placed, otherwise show all.
	useEffect(() => {
		if (!flow || nodes.length === 0) return;
		const focusIds = expandedLoaded
			? nodes
					.filter((n) => n.id.startsWith(`lan:${expandedKey}`))
					.map((n) => ({ id: n.id }))
			: undefined;
		const frame = requestAnimationFrame(() => {
			flow.fitView({ padding: 0.15, duration: 300, nodes: focusIds });
		});
		return () => cancelAnimationFrame(frame);
	}, [flow, nodes, expandedLoaded, expandedKey]);

	const neighbourCount = lans.reduce((n, l) => n + l.devices.length - 1, 0);
	const lanDeviceCount = allLans.reduce((n, l) => n + l.device_count, 0);
	const isEmpty = serial ? !focused : allLans.length === 0;

	return (
		<PageContainer className="flex flex-col gap-4">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<DevicePicker onPick={select} />
				{!isLoading && !isError && (
					<span className="text-sm text-gray-500">
						{serial ? (
							<>
								{lans.length} LAN{lans.length !== 1 ? "s" : ""} ·{" "}
								{neighbourCount} neighbour{neighbourCount !== 1 ? "s" : ""} ·{" "}
								<button
									type="button"
									onClick={() => select(null)}
									className="text-blue-600 hover:underline"
								>
									All LANs
								</button>
							</>
						) : (
							<>
								{allLans.length} LAN{allLans.length !== 1 ? "s" : ""} ·{" "}
								{lanDeviceCount} device{lanDeviceCount !== 1 ? "s" : ""}
							</>
						)}
					</span>
				)}
			</div>

			<div className="flex min-h-[600px] flex-1 flex-col gap-4 lg:flex-row">
				<Card className="relative min-h-[500px] flex-1 overflow-hidden">
					{isLoading ? (
						<div className="p-12 text-center text-gray-500">Loading...</div>
					) : isError ? (
						<div className="p-12 text-center text-gray-500">
							<Network className="mx-auto mb-4 h-12 w-12 text-gray-400" />
							<h3 className="mb-2 text-lg font-medium text-gray-900">
								Failed to load LANs
							</h3>
							<p>The API may be unreachable or older than this dashboard.</p>
						</div>
					) : isEmpty ? (
						<div className="p-12 text-center text-gray-500">
							<Network className="mx-auto mb-4 h-12 w-12 text-gray-400" />
							<h3 className="mb-2 text-lg font-medium text-gray-900">
								No LAN reported
							</h3>
							<p>
								{serial
									? "The device needs a smithd version that reports its gateway."
									: "Devices need a smithd version that reports their gateway."}
							</p>
						</div>
					) : (
						<ReactFlow
							nodes={nodes}
							edges={edges}
							nodeTypes={nodeTypes}
							onInit={setFlow}
							onNodeClick={(_, node) => {
								if (node.type === "router") {
									if (serial) return;
									const key = node.id.slice("lan:".length);
									expand(key === expandedKey ? null : key);
									return;
								}
								const { device } = node.data as DeviceData;
								if (device.serial_number !== serial)
									select(device.serial_number);
							}}
							nodesConnectable={false}
							onlyRenderVisibleElements
							fitView
							fitViewOptions={{ padding: 0.15 }}
							minZoom={0.05}
							proOptions={{ hideAttribution: true }}
						>
							<Background gap={24} size={1} />
							<Controls showInteractive={false} />
						</ReactFlow>
					)}
				</Card>

				{focused && (
					<PeerPanel
						lans={lans}
						device={focused}
						onPick={select}
						onClose={() => select(null)}
					/>
				)}
				{!serial && expandedLoaded && (
					<LanPanel
						lan={expanded}
						onPick={select}
						onClose={() => expand(null)}
					/>
				)}
			</div>
		</PageContainer>
	);
};

export default NetworkMapPage;
