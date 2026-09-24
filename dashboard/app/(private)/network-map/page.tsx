import {
	Background,
	Controls,
	type Edge,
	type Node,
	type NodeProps,
	ReactFlow,
	type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Card, PageContainer, SearchInput } from "@teton/smith-ui";
import { Cpu, Globe, Network, Router, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { type Lan, type LanDevice, useGetLans } from "@/app/api-client";

const DEVICE_W = 170;
const DEVICE_H = 44;
const GAP = 12;
const PAD = 16;
const HEADER = 40;
const LAN_COLS = 3;

type Site = { key: string; label: string; lans: Lan[] };

/** Groups LANs under the public IP most of their devices report. A LAN can
 *  span several public IPs (multi-WAN), but drawing it once reads better. */
const groupBySite = (lans: Lan[]): Site[] => {
	const sites = new Map<string, Site>();
	for (const lan of lans) {
		const tally = new Map<string, number>();
		for (const d of lan.devices) {
			const ip = d.public_ip ?? "unknown";
			tally.set(ip, (tally.get(ip) ?? 0) + 1);
		}
		const [ip] = [...tally.entries()].sort((a, b) => b[1] - a[1])[0] ?? [
			"unknown",
		];
		const name = lan.devices.find((d) => d.public_ip === ip)?.public_ip_name;
		const site = sites.get(ip) ?? {
			key: ip,
			label:
				ip === "unknown" ? "Unknown public IP" : name ? `${name} · ${ip}` : ip,
			lans: [],
		};
		site.lans.push(lan);
		sites.set(ip, site);
	}
	return [...sites.values()].sort((a, b) => a.label.localeCompare(b.label));
};

const lanSize = (count: number) => {
	const cols = Math.min(LAN_COLS, count);
	const rows = Math.ceil(count / LAN_COLS);
	return {
		width: PAD * 2 + cols * DEVICE_W + (cols - 1) * GAP,
		height: HEADER + PAD + rows * DEVICE_H + (rows - 1) * GAP,
	};
};

type DeviceData = {
	device: LanDevice;
	state: "normal" | "selected" | "peer" | "dim";
};
type GroupData = { title: string; subtitle?: string; dim?: boolean };

const SiteNode = ({ data }: NodeProps<Node<GroupData>>) => (
	<div className="h-full w-full rounded-xl border border-gray-300 bg-gray-50/60">
		<div className="flex items-center gap-2 px-4 py-2.5 text-sm font-semibold text-gray-700">
			<Globe className="h-4 w-4 text-gray-400" />
			{data.title}
		</div>
	</div>
);

const LanNode = ({ data }: NodeProps<Node<GroupData>>) => (
	<div
		className={`h-full w-full rounded-lg border bg-white transition-opacity ${
			data.dim ? "border-gray-200 opacity-40" : "border-blue-200"
		}`}
	>
		<div className="flex items-center gap-2 px-3 py-2 text-xs">
			<Router className="h-3.5 w-3.5 text-blue-500" />
			<span className="font-mono font-medium text-gray-800">{data.title}</span>
			<span className="truncate text-gray-400">{data.subtitle}</span>
		</div>
	</div>
);

const DeviceNode = ({ data }: NodeProps<Node<DeviceData>>) => {
	const { device, state } = data;
	const ring =
		state === "selected"
			? "border-blue-500 ring-2 ring-blue-200"
			: state === "peer"
				? "border-blue-300"
				: "border-gray-200";
	return (
		<div
			className={`flex h-full w-full cursor-pointer items-center gap-2 rounded-md border bg-white px-2.5 shadow-sm transition-opacity ${ring} ${
				state === "dim" ? "opacity-30" : ""
			}`}
		>
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

const nodeTypes = { site: SiteNode, lan: LanNode, device: DeviceNode };

const buildGraph = (sites: Site[], selectedId: number | null) => {
	const nodes: Node[] = [];
	const edges: Edge[] = [];
	const selectedLans = new Set(
		sites.flatMap((s) =>
			s.lans
				.filter((l) => l.devices.some((d) => d.id === selectedId))
				.map((l) => l.key),
		),
	);

	let siteY = 0;
	for (const site of sites) {
		let lanX = PAD;
		let siteHeight = 0;
		const siteId = `site:${site.key}`;
		nodes.push({
			id: siteId,
			type: "site",
			position: { x: 0, y: siteY },
			data: { title: site.label },
			draggable: false,
			selectable: false,
		});

		for (const lan of site.lans) {
			const size = lanSize(lan.devices.length);
			const lanId = `lan:${lan.key}`;
			const inSelection = selectedLans.has(lan.key);
			nodes.push({
				id: lanId,
				type: "lan",
				parentId: siteId,
				position: { x: lanX, y: HEADER },
				...size,
				style: size,
				data: {
					title: lan.network,
					subtitle: `gw ${lan.gateway_ip}`,
					dim: selectedId != null && !inSelection,
				},
				draggable: false,
				selectable: false,
			});

			lan.devices.forEach((device, i) => {
				const id = `${lanId}:${device.id}`;
				const state: DeviceData["state"] =
					selectedId == null
						? "normal"
						: device.id === selectedId
							? "selected"
							: inSelection
								? "peer"
								: "dim";
				nodes.push({
					id,
					type: "device",
					parentId: lanId,
					extent: "parent",
					position: {
						x: PAD + (i % LAN_COLS) * (DEVICE_W + GAP),
						y: HEADER + Math.floor(i / LAN_COLS) * (DEVICE_H + GAP),
					},
					width: DEVICE_W,
					height: DEVICE_H,
					style: { width: DEVICE_W, height: DEVICE_H },
					data: { device, state },
					draggable: false,
				});
				if (inSelection && device.id !== selectedId) {
					edges.push({
						id: `edge:${lanId}:${selectedId}:${device.id}`,
						source: `${lanId}:${selectedId}`,
						target: id,
						animated: true,
						style: { stroke: "#3b82f6", strokeDasharray: "4 4" },
					});
				}
			});

			lanX += size.width + GAP * 2;
			siteHeight = Math.max(siteHeight, size.height);
		}

		const siteNode = nodes.find((n) => n.id === siteId);
		if (siteNode) {
			siteNode.width = lanX - GAP * 2 + PAD;
			siteNode.height = HEADER + siteHeight + PAD;
			siteNode.style = { width: siteNode.width, height: siteNode.height };
		}
		siteY += HEADER + siteHeight + PAD + 32;
	}

	return { nodes, edges };
};

const PeerPanel = ({
	lans,
	device,
	onClose,
}: {
	lans: Lan[];
	device: LanDevice;
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
		<div className="divide-y divide-gray-100">
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
										<Link
											to={`/devices/${peer.serial_number}/network`}
											className="flex items-center justify-between gap-2 rounded px-2 py-1 text-sm hover:bg-gray-50"
										>
											<span className="flex min-w-0 items-center gap-2">
												<span
													className={`h-2 w-2 flex-shrink-0 rounded-full ${
														peer.online ? "bg-green-500" : "bg-gray-300"
													}`}
												/>
												<span className="truncate font-mono">
													{peer.serial_number}
												</span>
											</span>
											<span className="font-mono text-xs text-gray-500">
												{peer.address}
											</span>
										</Link>
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

/** Devices grouped by the LAN they share (same gateway MAC and subnet), and
 *  LANs by the public IP they sit behind. */
const NetworkMapPage = () => {
	const [searchParams, setSearchParams] = useSearchParams();
	const [search, setSearch] = useState("");
	const [sharedOnly, setSharedOnly] = useState(true);
	const [flow, setFlow] = useState<ReactFlowInstance | null>(null);
	const { data: lans = [], isLoading } = useGetLans({
		query: { select: (data) => data.lans },
	});

	const focusSerial = searchParams.get("device");
	const selected = useMemo(
		() =>
			lans
				.flatMap((l) => l.devices)
				.find((d) => d.serial_number === focusSerial) ?? null,
		[lans, focusSerial],
	);

	const visibleLans = useMemo(() => {
		const term = search.trim().toLowerCase();
		return lans.filter((lan) => {
			// Keep the focused device's LAN even when it's alone on it.
			if (selected && lan.devices.some((d) => d.id === selected.id))
				return true;
			if (sharedOnly && lan.devices.length < 2) return false;
			if (!term) return true;
			return (
				lan.network.includes(term) ||
				lan.devices.some(
					(d) =>
						d.serial_number.toLowerCase().includes(term) ||
						d.address.includes(term),
				)
			);
		});
	}, [lans, search, sharedOnly, selected]);

	const { nodes, edges } = useMemo(
		() => buildGraph(groupBySite(visibleLans), selected?.id ?? null),
		[visibleLans, selected],
	);

	const select = (serial: string | null) => {
		const params = new URLSearchParams(searchParams);
		if (serial) params.set("device", serial);
		else params.delete("device");
		setSearchParams(params, { replace: true });
	};

	const selectedLans = useMemo(
		() =>
			selected
				? lans.filter((l) => l.devices.some((d) => d.id === selected.id))
				: [],
		[lans, selected],
	);

	// Refit only when what's shown changes, so manual pan/zoom isn't undone.
	useEffect(() => {
		if (!flow || visibleLans.length === 0) return;
		const frame = requestAnimationFrame(() => {
			flow.fitView({
				padding: selectedLans.length ? 0.4 : 0.1,
				duration: 300,
				nodes: selectedLans.length
					? selectedLans.map((l) => ({ id: `lan:${l.key}` }))
					: undefined,
			});
		});
		return () => cancelAnimationFrame(frame);
	}, [flow, visibleLans, selectedLans]);

	return (
		<PageContainer className="flex flex-col gap-4">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<SearchInput
					value={search}
					onChange={setSearch}
					placeholder="Search serial, IP or subnet..."
					className="w-full sm:w-72"
				/>
				<div className="flex items-center gap-4 text-sm text-gray-500">
					<label className="flex cursor-pointer items-center gap-2">
						<input
							type="checkbox"
							checked={sharedOnly}
							onChange={(e) => setSharedOnly(e.target.checked)}
						/>
						Only LANs with 2+ devices
					</label>
					<span>
						{isLoading
							? "Loading..."
							: `${visibleLans.length} LAN${visibleLans.length !== 1 ? "s" : ""}`}
					</span>
				</div>
			</div>

			<div className="flex min-h-[600px] flex-1 flex-col gap-4 lg:flex-row">
				<Card className="relative min-h-[500px] flex-1 overflow-hidden">
					{!isLoading && visibleLans.length === 0 ? (
						<div className="p-12 text-center text-gray-500">
							<Network className="mx-auto mb-4 h-12 w-12 text-gray-400" />
							<h3 className="mb-2 text-lg font-medium text-gray-900">
								No LANs to show
							</h3>
							<p>
								{lans.length === 0
									? "Devices report their gateway once they run a smithd version with LAN detection."
									: "Try clearing the search or showing single-device LANs."}
							</p>
						</div>
					) : (
						<ReactFlow
							nodes={nodes}
							edges={edges}
							nodeTypes={nodeTypes}
							onInit={setFlow}
							onNodeClick={(_, node) => {
								if (node.type !== "device") return;
								const { device } = node.data as DeviceData;
								select(
									device.id === selected?.id ? null : device.serial_number,
								);
							}}
							onPaneClick={() => select(null)}
							nodesConnectable={false}
							fitView
							fitViewOptions={{ padding: 0.1 }}
							minZoom={0.1}
							proOptions={{ hideAttribution: true }}
						>
							<Background gap={24} size={1} />
							<Controls showInteractive={false} />
						</ReactFlow>
					)}
				</Card>

				{selected && (
					<PeerPanel
						lans={selectedLans}
						device={selected}
						onClose={() => select(null)}
					/>
				)}
			</div>
		</PageContainer>
	);
};

export default NetworkMapPage;
