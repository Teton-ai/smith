"use client";

import { useMutation } from "@tanstack/react-query";
import {
	Badge,
	Button,
	Panel,
	SECTION_THEMES,
	SearchInput,
} from "@teton/smith-ui";
import { Eye, EyeOff, Radar, RefreshCw, Wifi, WifiOff } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
	type ConfiguredNetwork,
	useGetConfiguredNetworksForDevice,
	useGetWifiScanForDevice,
	type WifiScanResult,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";

const MASK = "••••••••••••";

const filterFieldClass =
	"px-2 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900";

interface WifiPanelProps {
	serial: string;
}

const WifiPanel = ({ serial }: WifiPanelProps) => {
	const [revealedIds, setRevealedIds] = useState<Set<string>>(new Set());
	const [syncing, setSyncing] = useState(false);
	const dispatchedAt = useRef<Date | null>(null);
	const [scanSyncing, setScanSyncing] = useState(false);
	const scanDispatchedAt = useRef<Date | null>(null);
	const [scanQuery, setScanQuery] = useState("");
	const [bandFilter, setBandFilter] = useState("all");
	const [securityFilter, setSecurityFilter] = useState("all");

	const {
		data: profiles,
		isLoading,
		isError,
	} = useGetConfiguredNetworksForDevice(serial, {
		query: { refetchInterval: syncing ? 3000 : 30000 },
	});

	const currentNetwork = profiles?.find((p) => p.is_active);

	const { data: scanResults, isLoading: isScanLoading } =
		useGetWifiScanForDevice(serial, {
			query: { refetchInterval: scanSyncing ? 3000 : false },
		});

	const fetcher = useClientMutator<void>();
	const { mutate: dispatchRefresh, isPending: isDispatching } = useMutation({
		mutationFn: () =>
			fetcher({
				url: `/devices/${serial}/commands`,
				method: "POST",
				data: [
					{ id: -6, command: "ReportNMProfiles", continue_on_error: false },
				],
			}),
		onSuccess: () => {
			setSyncing(true);
			dispatchedAt.current = new Date();
		},
	});

	const { mutate: dispatchScan, isPending: isScanDispatching } = useMutation({
		mutationFn: () =>
			fetcher({
				url: `/devices/${serial}/commands`,
				method: "POST",
				data: [{ id: -7, command: "WifiScan", continue_on_error: false }],
			}),
		onSuccess: () => {
			setScanSyncing(true);
			scanDispatchedAt.current = new Date();
		},
	});

	// Fast path: clear syncing when the DB rows confirm the device responded.
	useEffect(() => {
		if (
			syncing &&
			dispatchedAt.current !== null &&
			profiles?.some((p) => new Date(p.updated_at) > dispatchedAt.current!)
		) {
			setSyncing(false);
			dispatchedAt.current = null;
		}
	}, [profiles, syncing]);

	// Timeout fallback: covers devices with no configured profiles (empty array
	// has no updated_at to compare) and offline devices.
	useEffect(() => {
		if (!syncing) return;
		const timer = setTimeout(() => {
			setSyncing(false);
			dispatchedAt.current = null;
		}, 45_000);
		return () => clearTimeout(timer);
	}, [syncing]);

	useEffect(() => {
		if (
			scanSyncing &&
			scanDispatchedAt.current !== null &&
			scanResults?.some(
				(r) => new Date(r.scanned_at) > scanDispatchedAt.current!,
			)
		) {
			setScanSyncing(false);
			scanDispatchedAt.current = null;
		}
	}, [scanResults, scanSyncing]);

	useEffect(() => {
		if (!scanSyncing) return;
		const timer = setTimeout(() => {
			setScanSyncing(false);
			scanDispatchedAt.current = null;
		}, 45_000);
		return () => clearTimeout(timer);
	}, [scanSyncing]);

	const securityOptions = useMemo(
		() =>
			[...new Set((scanResults ?? []).map((r) => r.security ?? "Open"))].sort(),
		[scanResults],
	);

	const filteredResults = useMemo(() => {
		const query = scanQuery.trim().toLowerCase();
		return (scanResults ?? []).filter((r) => {
			if (
				query &&
				!r.ssid?.toLowerCase().includes(query) &&
				!r.bssid.toLowerCase().includes(query)
			) {
				return false;
			}
			if (bandFilter !== "all" && r.band !== bandFilter) return false;
			if (securityFilter !== "all" && (r.security ?? "Open") !== securityFilter)
				return false;
			return true;
		});
	}, [scanResults, scanQuery, bandFilter, securityFilter]);

	const toggleReveal = (profileName: string) => {
		setRevealedIds((prev) => {
			const next = new Set(prev);
			if (next.has(profileName)) {
				next.delete(profileName);
			} else {
				next.add(profileName);
			}
			return next;
		});
	};

	const isBusy = isLoading || isDispatching || syncing;

	return (
		<Panel title="WiFi" icon={Wifi} theme={SECTION_THEMES.orange}>
			<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
				<div>
					{/* Last checked */}
					<div className="flex items-center justify-between gap-2 mb-3">
						{isError ? (
							<p className="text-sm text-red-500">
								Failed to load WiFi profiles.
							</p>
						) : (
							<p className="text-sm text-gray-500">
								{profiles && profiles.length > 0
									? `Last checked ${new Date(Math.max(...profiles.map((p) => new Date(p.updated_at).getTime()))).toLocaleString()}`
									: "Never checked"}
							</p>
						)}
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							loading={isDispatching || syncing}
							onClick={() => dispatchRefresh()}
							icon={<RefreshCw className="w-4 h-4" />}
						>
							{syncing ? "Syncing..." : "Refresh"}
						</Button>
					</div>

					{/* Current network */}
					<div className="mb-4 pb-4 border-b border-gray-100">
						<p className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
							Current network
						</p>
						{isBusy && !currentNetwork ? (
							<div className="h-4 w-32 bg-gray-100 rounded animate-pulse" />
						) : currentNetwork ? (
							<div className="flex items-center gap-2">
								<Wifi className="w-4 h-4 text-green-500 flex-shrink-0" />
								<span className="text-sm font-medium text-gray-900">
									{currentNetwork.ssid}
								</span>
								<Badge variant="green" pill>
									Connected
								</Badge>
							</div>
						) : (
							<div className="flex items-center gap-2 text-gray-500">
								<WifiOff className="w-4 h-4 flex-shrink-0" />
								<span className="text-sm">Disconnected</span>
							</div>
						)}
					</div>

					{/* Configured profiles */}
					<div>
						<p className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
							Configured profiles
						</p>
						{isBusy && (!profiles || profiles.length === 0) ? (
							<div className="space-y-2">
								{[1, 2, 3].map((i) => (
									<div
										key={i}
										className="h-4 bg-gray-100 rounded animate-pulse"
									/>
								))}
							</div>
						) : !profiles || profiles.length === 0 ? (
							<p className="text-sm text-gray-500">No profiles reported yet.</p>
						) : (
							<div className="divide-y divide-gray-100">
								{profiles.map((profile: ConfiguredNetwork) => {
									const revealed = revealedIds.has(profile.profile_name);
									return (
										<div
											key={profile.profile_name}
											className="py-3 flex flex-col gap-1"
										>
											<div className="flex items-center justify-between gap-2">
												<div className="flex flex-col min-w-0">
													<span className="text-sm font-medium text-gray-900 truncate">
														{profile.profile_name}
													</span>
													{profile.ssid && (
														<span className="text-xs text-gray-500 font-mono truncate">
															{profile.ssid}
														</span>
													)}
												</div>
												<div className="flex items-center gap-2 flex-shrink-0">
													{profile.is_active && (
														<Badge variant="green" pill>
															Active
														</Badge>
													)}
												</div>
											</div>
											{profile.password && (
												<div className="flex items-center gap-2">
													<span className="font-mono text-sm text-gray-700">
														{revealed ? profile.password : MASK}
													</span>
													<button
														type="button"
														onClick={() => toggleReveal(profile.profile_name)}
														className="text-gray-400 hover:text-gray-600"
														aria-label={`${revealed ? "Hide" : "Reveal"} password for ${profile.profile_name}`}
														aria-pressed={revealed}
													>
														{revealed ? (
															<EyeOff className="w-3.5 h-3.5" />
														) : (
															<Eye className="w-3.5 h-3.5" />
														)}
													</button>
												</div>
											)}
										</div>
									);
								})}
							</div>
						)}
					</div>
				</div>

				{/* Scan results */}
				<div className="border-t border-gray-100 pt-4 lg:border-t-0 lg:pt-0 lg:border-l lg:border-gray-100 lg:pl-6">
					<div className="flex items-center justify-between gap-2 mb-3">
						<p className="text-sm text-gray-500">
							{scanResults && scanResults.length > 0
								? `Last checked ${new Date(Math.max(...scanResults.map((r) => new Date(r.scanned_at).getTime()))).toLocaleString()}`
								: "Never scanned"}
						</p>
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							loading={isScanDispatching || scanSyncing}
							onClick={() => dispatchScan()}
							icon={<Radar className="w-4 h-4" />}
						>
							{scanSyncing ? "Scanning..." : "Scan WiFi"}
						</Button>
					</div>
					<p className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
						Scan results
					</p>
					{(isScanLoading || scanSyncing) &&
					(!scanResults || scanResults.length === 0) ? (
						<div className="space-y-2">
							{[1, 2, 3].map((i) => (
								<div
									key={i}
									className="h-4 bg-gray-100 rounded animate-pulse"
								/>
							))}
						</div>
					) : !scanResults || scanResults.length === 0 ? null : (
						<>
							<div className="flex items-center gap-2 mb-3">
								<SearchInput
									value={scanQuery}
									onChange={setScanQuery}
									placeholder="Filter by SSID or BSSID..."
									className="flex-1 min-w-0"
								/>
								<select
									value={bandFilter}
									onChange={(e) => setBandFilter(e.target.value)}
									aria-label="Filter by band"
									className={filterFieldClass}
								>
									<option value="all">All bands</option>
									<option value="2.4 GHz">2.4 GHz</option>
									<option value="5 GHz">5 GHz</option>
								</select>
								<select
									value={securityFilter}
									onChange={(e) => setSecurityFilter(e.target.value)}
									aria-label="Filter by security"
									className={filterFieldClass}
								>
									<option value="all">All security</option>
									{securityOptions.map((s) => (
										<option key={s} value={s}>
											{s}
										</option>
									))}
								</select>
							</div>
							{filteredResults.length === 0 ? (
								<p className="text-sm text-gray-500">
									No networks match the filters.
								</p>
							) : (
								<div className="overflow-x-auto">
									<table className="min-w-full divide-y divide-gray-200">
										<thead>
											<tr>
												<th
													scope="col"
													className="py-2 pr-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
												>
													Network
												</th>
												<th
													scope="col"
													className="px-2 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
												>
													Band
												</th>
												<th
													scope="col"
													className="px-2 py-2 text-right text-xs font-medium text-gray-500 uppercase tracking-wider"
												>
													Signal
												</th>
												<th
													scope="col"
													className="px-2 py-2 text-right text-xs font-medium text-gray-500 uppercase tracking-wider"
												>
													Rate
												</th>
												<th
													scope="col"
													className="pl-2 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
												>
													Security
												</th>
											</tr>
										</thead>
										<tbody className="divide-y divide-gray-100">
											{filteredResults.map((result: WifiScanResult) => (
												<tr key={result.bssid}>
													<td className="py-2 pr-2 max-w-0 w-full">
														<div className="flex flex-col min-w-0">
															{result.ssid ? (
																<span className="text-sm font-medium text-gray-900 truncate">
																	{result.ssid}
																</span>
															) : (
																<span className="text-sm italic text-gray-400 truncate">
																	&lt;hidden&gt;
																</span>
															)}
															<span className="text-xs text-gray-500 font-mono truncate">
																{result.bssid}
															</span>
														</div>
													</td>
													<td className="px-2 py-2 text-xs text-gray-500 whitespace-nowrap">
														{result.band ?? "—"}
													</td>
													<td className="px-2 py-2 text-xs text-gray-500 text-right whitespace-nowrap">
														{result.signal != null ? `${result.signal}%` : "—"}
													</td>
													<td className="px-2 py-2 text-xs text-gray-500 text-right whitespace-nowrap">
														{result.rate != null ? `${result.rate} Mbps` : "—"}
													</td>
													<td className="pl-2 py-2 whitespace-nowrap">
														<Badge variant="gray" pill>
															{result.security ?? "Open"}
														</Badge>
													</td>
												</tr>
											))}
										</tbody>
									</table>
								</div>
							)}
						</>
					)}
				</div>
			</div>
		</Panel>
	);
};

export default WifiPanel;
