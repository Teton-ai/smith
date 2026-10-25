import { Badge, Card, PageContainer, SearchInput } from "@teton/smith-ui";
import { ChevronDown, ChevronRight, Network } from "lucide-react";
import moment from "moment";
import { Fragment, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
	type LanSummary,
	useGetLan,
	useGetLans,
	useGetLansForDevice,
} from "@/app/api-client";

const REFRESH_MS = 30_000;
const COLUMNS = 5;

type Site = {
	name: string;
	lans: LanSummary[];
	online: number;
	total: number;
	lastSeen?: number;
	publicIps: string[];
};

const lanParams = (key: string) => {
	const [gateway_mac = "", network = ""] = key.split("|");
	return { gateway_mac, network };
};

const siteName = (lan: LanSummary) => {
	if (lan.departments.length > 0) {
		return lan.departments.join(", ");
	}

	return lan.public_ip_name ?? lan.public_ip ?? "Unknown site";
};

const matches = (lan: LanSummary, term: string) =>
	[
		...lan.departments,
		lan.network,
		lan.gateway_ip,
		lan.gateway_mac,
		lan.public_ip,
		lan.public_ip_name,
	].some((value) => value?.toLowerCase().includes(term));

const groupSites = (lans: LanSummary[]): Site[] => {
	const groups = new Map<string, LanSummary[]>();
	for (const lan of lans) {
		const name = siteName(lan);
		groups.set(name, [...(groups.get(name) ?? []), lan]);
	}

	const sites = [...groups].map(([name, siteLans]) => {
		const seen = siteLans.flatMap((lan) =>
			lan.last_seen ? [Date.parse(lan.last_seen)] : [],
		);

		return {
			name,
			lans: siteLans,
			online: siteLans.reduce((n, lan) => n + lan.online_count, 0),
			total: siteLans.reduce((n, lan) => n + lan.device_count, 0),
			lastSeen: seen.length > 0 ? Math.max(...seen) : undefined,
			publicIps: [...new Set(siteLans.flatMap((lan) => lan.public_ip ?? []))],
		};
	});

	return sites.sort(
		(a, b) => b.total - b.online - (a.total - a.online) || b.total - a.total,
	);
};

const OnlineBadge = ({ online, total }: { online: number; total: number }) => (
	<Badge variant={online === total ? "green" : online === 0 ? "red" : "yellow"}>
		{online} / {total}
	</Badge>
);

const lastSeenText = (online: number, lastSeen?: string | number) => {
	if (online > 0) {
		return "Now";
	}

	return lastSeen ? moment(lastSeen).fromNow() : "Never";
};

const Chevron = ({ open }: { open: boolean }) =>
	open ? (
		<ChevronDown className="h-4 w-4 text-gray-400" />
	) : (
		<ChevronRight className="h-4 w-4 text-gray-400" />
	);

const LanDevices = ({ lanKey, serial }: { lanKey: string; serial: string }) => {
	const { data: lan, isLoading } = useGetLan(lanParams(lanKey), {
		query: { refetchInterval: REFRESH_MS },
	});

	if (isLoading || !lan) {
		return (
			<p className="px-6 py-3 text-sm text-gray-500">Loading devices...</p>
		);
	}

	const devices = [...lan.devices].sort(
		(a, b) => Number(a.online) - Number(b.online),
	);

	return (
		<ul className="divide-y divide-gray-100 bg-gray-50 pl-16 pr-6">
			{devices.map((device) => (
				<li
					key={`${device.id}:${device.interface}`}
					className="flex items-center justify-between gap-4 py-2 text-sm"
				>
					<span className="flex min-w-0 items-center gap-2">
						<span
							className={`h-2 w-2 flex-shrink-0 rounded-full ${
								device.online ? "bg-green-500" : "bg-red-500"
							}`}
						/>
						<Link
							to={`/devices/${device.serial_number}/network`}
							className={`truncate font-mono hover:text-blue-600 ${
								device.serial_number === serial
									? "font-semibold text-blue-700"
									: "text-gray-900"
							}`}
						>
							{device.serial_number}
						</Link>
					</span>
					<span className="flex items-center gap-4 text-gray-500">
						<span className="font-mono">
							{device.address} · {device.interface}
						</span>
						<span className="w-28 text-right">
							{device.online
								? "Online"
								: device.last_seen
									? moment(device.last_seen).fromNow()
									: "Never"}
						</span>
					</span>
				</li>
			))}
		</ul>
	);
};

const NetworkMapPage = () => {
	const [searchParams, setSearchParams] = useSearchParams();
	const [search, setSearch] = useState("");
	const serial = searchParams.get("device") ?? "";
	const expandedSite = searchParams.get("site");
	const expandedKey = searchParams.get("lan");

	const {
		data: lans = [],
		isLoading,
		isError,
	} = useGetLans({
		query: { select: (data) => data.lans, refetchInterval: REFRESH_MS },
	});
	const { data: deviceLanKeys } = useGetLansForDevice(serial, {
		query: {
			enabled: serial.length > 0,
			select: (data) => new Set(data.lans.map((lan) => lan.key)),
		},
	});

	const sites = useMemo(() => {
		const term = search.trim().toLowerCase();
		const visible = lans.filter((lan) => {
			if (deviceLanKeys && !deviceLanKeys.has(lan.key)) {
				return false;
			}

			return term === "" || matches(lan, term);
		});

		return groupSites(visible);
	}, [lans, search, deviceLanKeys]);

	const lanCount = sites.reduce((n, site) => n + site.lans.length, 0);
	const offline = sites.reduce((n, site) => n + site.total - site.online, 0);

	const isOpen = (site: Site) =>
		site.name === expandedSite ||
		site.lans.some((lan) => lan.key === expandedKey);

	const toggleSite = (site: Site) => {
		const params = new URLSearchParams(searchParams);
		if (isOpen(site)) {
			params.delete("site");
			params.delete("lan");
		} else {
			params.set("site", site.name);
		}
		setSearchParams(params);
	};

	const toggleLan = (key: string) => {
		const params = new URLSearchParams(searchParams);
		if (key === expandedKey) {
			params.delete("lan");
		} else {
			params.set("lan", key);
		}
		setSearchParams(params);
	};

	const clearDevice = () => {
		const params = new URLSearchParams(searchParams);
		params.delete("device");
		setSearchParams(params);
	};

	return (
		<PageContainer className="flex flex-col gap-4">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<SearchInput
					value={search}
					onChange={setSearch}
					placeholder="Department, subnet, gateway or public IP..."
					className="w-full sm:w-96"
					slashToFocus
				/>
				{!isLoading && !isError && (
					<span className="text-sm text-gray-500">
						{serial && (
							<>
								LANs of <span className="font-mono">{serial}</span> ·{" "}
								<button
									type="button"
									onClick={clearDevice}
									className="text-blue-600 hover:underline"
								>
									All LANs
								</button>{" "}
								·{" "}
							</>
						)}
						{sites.length} site{sites.length !== 1 ? "s" : ""} · {lanCount} LAN
						{lanCount !== 1 ? "s" : ""} · {offline} offline device
						{offline !== 1 ? "s" : ""}
					</span>
				)}
			</div>

			<Card className="overflow-hidden">
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
				) : sites.length === 0 ? (
					<div className="p-12 text-center text-gray-500">
						<Network className="mx-auto mb-4 h-12 w-12 text-gray-400" />
						<h3 className="mb-2 text-lg font-medium text-gray-900">
							No LAN reported
						</h3>
						<p>Devices need a smithd version that reports their gateway.</p>
					</div>
				) : (
					<div className="overflow-x-auto">
						<table className="min-w-full divide-y divide-gray-200">
							<thead className="bg-gray-50">
								<tr>
									{["Site", "LANs", "Public IP", "Online", "Last seen"].map(
										(title) => (
											<th
												key={title}
												scope="col"
												className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500"
											>
												{title}
											</th>
										),
									)}
								</tr>
							</thead>
							<tbody className="divide-y divide-gray-200 bg-white">
								{sites.map((site) => {
									const open = isOpen(site);

									return (
										<Fragment key={site.name}>
											<tr
												onClick={() => toggleSite(site)}
												className={`cursor-pointer ${
													open ? "bg-blue-50" : "hover:bg-gray-50"
												}`}
											>
												<td className="px-4 py-3 text-sm font-medium text-gray-900">
													<span className="flex items-center gap-2">
														<Chevron open={open} />
														{site.name}
													</span>
												</td>
												<td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
													{site.lans.length}
												</td>
												<td className="px-4 py-3 text-sm text-gray-600">
													{site.publicIps.join(", ") || "—"}
												</td>
												<td className="whitespace-nowrap px-4 py-3">
													<OnlineBadge
														online={site.online}
														total={site.total}
													/>
												</td>
												<td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
													{lastSeenText(site.online, site.lastSeen)}
												</td>
											</tr>
											{open &&
												site.lans.map((lan) => {
													const expanded = lan.key === expandedKey;

													return (
														<Fragment key={lan.key}>
															<tr
																onClick={() => toggleLan(lan.key)}
																className="cursor-pointer bg-gray-50 hover:bg-gray-100"
															>
																<td className="whitespace-nowrap py-2 pl-10 pr-4 font-mono text-sm text-gray-700">
																	<span className="flex items-center gap-2">
																		<Chevron open={expanded} />
																		{lan.network} · {lan.gateway_ip}
																	</span>
																</td>
																<td />
																<td className="whitespace-nowrap px-4 py-2 text-sm text-gray-600">
																	{lan.public_ip ?? "—"}
																</td>
																<td className="whitespace-nowrap px-4 py-2">
																	<OnlineBadge
																		online={lan.online_count}
																		total={lan.device_count}
																	/>
																</td>
																<td className="whitespace-nowrap px-4 py-2 text-sm text-gray-600">
																	{lastSeenText(
																		lan.online_count,
																		lan.last_seen,
																	)}
																</td>
															</tr>
															{expanded && (
																<tr>
																	<td colSpan={COLUMNS} className="p-0">
																		<LanDevices
																			lanKey={lan.key}
																			serial={serial}
																		/>
																	</td>
																</tr>
															)}
														</Fragment>
													);
												})}
										</Fragment>
									);
								})}
							</tbody>
						</table>
					</div>
				)}
			</Card>
		</PageContainer>
	);
};

export default NetworkMapPage;
