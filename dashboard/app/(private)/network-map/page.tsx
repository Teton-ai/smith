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
	useGetDevices,
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

type DeviceData = { device: LanDevice; focused: boolean };
type RouterData = { lan: Lan };

// Edges run center to center, so each node gets one invisible centered handle.
const centerHandle = {
	top: "50%",
	left: "50%",
	transform: "translate(-50%, -50%)",
	opacity: 0,
	pointerEvents: "none" as const,
};

const RouterNode = ({ data }: NodeProps<Node<RouterData>>) => {
	const { lan } = data;
	const publicIp = lan.devices.find((d) => d.public_ip);
	return (
		<div className="flex h-full w-full items-center gap-3 rounded-xl border-2 border-blue-300 bg-blue-50 px-3 shadow-sm">
			<Handle
				type="source"
				position={Position.Top}
				style={centerHandle}
				isConnectable={false}
			/>
			<Router className="h-6 w-6 flex-shrink-0 text-blue-500" />
			<div className="min-w-0 text-xs">
				<div className="font-mono font-semibold text-gray-900">
					{lan.network}
				</div>
				<div className="truncate font-mono text-gray-500">
					gw {lan.gateway_ip}
				</div>
				{publicIp && (
					<div className="truncate text-gray-500">
						{publicIp.public_ip_name
							? `${publicIp.public_ip_name} · ${publicIp.public_ip}`
							: publicIp.public_ip}
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

const buildGraph = (lans: Lan[], focusedId: number) => {
	const nodes: Node[] = [];
	const edges: Edge[] = [];
	let cx = 0;

	for (const lan of lans) {
		const devices = [...lan.devices].sort(
			(a, b) => Number(b.id === focusedId) - Number(a.id === focusedId),
		);
		const { points, rx } = ringLayout(devices.length);
		cx += rx + DEVICE_W / 2;
		const routerId = `lan:${lan.key}`;
		nodes.push({
			id: routerId,
			type: "router",
			position: { x: cx - ROUTER_W / 2, y: -ROUTER_H / 2 },
			width: ROUTER_W,
			height: ROUTER_H,
			style: { width: ROUTER_W, height: ROUTER_H },
			data: { lan },
			draggable: false,
			selectable: false,
		});

		devices.forEach((device, i) => {
			const id = `${routerId}:${device.id}`;
			const focused = device.id === focusedId;
			nodes.push({
				id,
				type: "device",
				position: {
					x: cx + points[i].x - DEVICE_W / 2,
					y: points[i].y - DEVICE_H / 2,
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

		cx += rx + DEVICE_W / 2 + LAN_GAP;
	}

	return { nodes, edges };
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
									<li key={peer.id}>
										<button
											type="button"
											onClick={() => onPick(peer.serial_number)}
											className="flex w-full items-center justify-between gap-2 rounded px-2 py-1 text-sm hover:bg-gray-50"
										>
											<span className="flex min-w-0 items-center gap-2">
												<span
													className={`h-2 w-2 flex-shrink-0 rounded-full ${
														peer.online ? "bg-green-500" : "bg-gray-300"
													}`}
												/>
												<span className="truncate font-mono text-gray-900">
													{peer.serial_number}
												</span>
											</span>
											<span className="font-mono text-xs text-gray-600">
												{peer.address}
											</span>
										</button>
									</li>
								))}
							</ul>
						)}
					</div>
				);
			})}
		</div>
	</Card>
);

/** One device's LANs (same gateway MAC and subnet) with its neighbours around
 *  each gateway. Scoped to a device because the whole fleet is too large to
 *  fetch or draw at once; clicking a neighbour moves the focus to it. */
const NetworkMapPage = () => {
	const [searchParams, setSearchParams] = useSearchParams();
	const [flow, setFlow] = useState<ReactFlowInstance | null>(null);
	const serial = searchParams.get("device") ?? "";

	const { data: lans = [], isLoading } = useGetLansForDevice(serial, {
		query: { enabled: serial.length > 0, select: (data) => data.lans },
	});

	const focused = useMemo(
		() =>
			lans.flatMap((l) => l.devices).find((d) => d.serial_number === serial) ??
			null,
		[lans, serial],
	);

	const { nodes, edges } = useMemo(
		() => (focused ? buildGraph(lans, focused.id) : { nodes: [], edges: [] }),
		[lans, focused],
	);

	const select = (next: string | null) => {
		const params = new URLSearchParams(searchParams);
		if (next) params.set("device", next);
		else params.delete("device");
		setSearchParams(params);
	};

	useEffect(() => {
		if (!flow || nodes.length === 0) return;
		const frame = requestAnimationFrame(() => {
			flow.fitView({ padding: 0.15, duration: 300 });
		});
		return () => cancelAnimationFrame(frame);
	}, [flow, nodes]);

	const deviceCount = lans.reduce((n, l) => n + l.devices.length - 1, 0);

	return (
		<PageContainer className="flex flex-col gap-4">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<DevicePicker onPick={select} />
				{serial && !isLoading && (
					<span className="text-sm text-gray-500">
						{lans.length} LAN{lans.length !== 1 ? "s" : ""} · {deviceCount}{" "}
						neighbour{deviceCount !== 1 ? "s" : ""}
					</span>
				)}
			</div>

			<div className="flex min-h-[600px] flex-1 flex-col gap-4 lg:flex-row">
				<Card className="relative min-h-[500px] flex-1 overflow-hidden">
					{!serial || (!isLoading && !focused) ? (
						<div className="p-12 text-center text-gray-500">
							<Network className="mx-auto mb-4 h-12 w-12 text-gray-400" />
							<h3 className="mb-2 text-lg font-medium text-gray-900">
								{serial ? "No LAN reported" : "Pick a device"}
							</h3>
							<p>
								{serial
									? "The device needs a smithd version that reports its gateway."
									: "Search for a device to see the LANs it's on and its neighbours."}
							</p>
						</div>
					) : isLoading ? (
						<div className="p-12 text-center text-gray-500">Loading...</div>
					) : (
						<ReactFlow
							nodes={nodes}
							edges={edges}
							nodeTypes={nodeTypes}
							onInit={setFlow}
							onNodeClick={(_, node) => {
								if (node.type !== "device") return;
								const { device } = node.data as DeviceData;
								if (device.serial_number !== serial)
									select(device.serial_number);
							}}
							nodesConnectable={false}
							fitView
							fitViewOptions={{ padding: 0.15 }}
							minZoom={0.1}
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
			</div>
		</PageContainer>
	);
};

export default NetworkMapPage;
