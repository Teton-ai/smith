"use client";

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
	Badge,
	Button,
	Panel,
	SECTION_THEMES,
	SearchInput,
	Toast,
	type ToastState,
} from "@teton/smith-ui";
import { isAxiosError } from "axios";
import {
	ArrowLeft,
	ChevronDown,
	ChevronRight,
	ChevronUp,
	Eye,
	EyeOff,
	Plus,
	Radar,
	RefreshCw,
	Trash2,
	Wifi,
	WifiOff,
	X,
} from "lucide-react";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import {
	type ConfiguredNetwork,
	type Device,
	type DeviceNetworkIntent,
	getGetDeviceInfoQueryKey,
	getGetDeviceIntentQueryKey,
	getGetNetworksQueryKey,
	useApplyDeviceIntent,
	useCreateDeviceIntent,
	useDeleteDeviceIntent,
	useDeleteNetworkById,
	useGetConfiguredNetworksForDevice,
	useGetDeviceInfo,
	useGetDeviceIntent,
	useGetNetworks,
	useGetWifiScanForDevice,
	useUpdateDeviceIntent,
	type WifiScanResult,
} from "@/app/api-client";
import {
	useClientMutator,
	useClientMutatorWithStatus,
} from "@/app/api-client-mutator";
import { Tooltip } from "@/app/components/tooltip";
import {
	groupScanResults,
	type ScanGroup,
	SignalBar,
} from "@/app/utils/wifiScan";

const MASK = "••••••••••••";

const filterFieldClass =
	"px-2 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900";

const getProfileTimestamp = (p: ConfiguredNetwork) => p.updated_at;
const getScanTimestamp = (r: WifiScanResult) => r.scanned_at;

type SecurityClass = "open" | "wep" | "psk" | "sae" | "owe" | "enterprise";

/** A scan security string can name more than one mode at once (nmcli reports
 *  transitional APs as e.g. "WPA1 WPA2"), so a group can match more than one
 *  class. WEP gets its own class: it's not WPA-PSK, and NM key-mgmt can't
 *  distinguish it from "open" on the catalog side (see catalogSecurityClass),
 *  so a WEP AP must never badge-match a WPA-PSK-configured network. */
function scanSecurityClasses(security: string | null): Set<SecurityClass> {
	if (!security) return new Set<SecurityClass>(["open"]);
	const s = security.toUpperCase();
	const classes = new Set<SecurityClass>();
	if (s.includes("OWE")) classes.add("owe");
	if (s.includes("802.1X") || s.includes("EAP")) classes.add("enterprise");
	if (s.includes("SAE") || s.includes("WPA3")) classes.add("sae");
	if (s.includes("WPA1") || s.includes("WPA2")) classes.add("psk");
	if (s.includes("WEP")) classes.add("wep");
	if (classes.size === 0) classes.add("open");
	return classes;
}

/** Mirrors api's `map_key_mgmt` (api/src/home.rs). */
function catalogSecurityClass(securityType?: string): SecurityClass | null {
	switch (securityType) {
		case "open":
			return "open";
		case "wpa-psk":
			return "psk";
		case "sae":
			return "sae";
		case "owe":
			return "owe";
		case "wpa-eap":
			return "enterprise";
		default:
			return null;
	}
}

/** SSID must match; security only narrows further when we actually know the
 *  catalog network's security_type (older rows predate that column and fall
 *  back to SSID-only matching). */
function recordMatchesGroup(
	record: { ssid?: string; security_type?: string },
	group: ScanGroup,
	groupClasses: Set<SecurityClass>,
): boolean {
	if (group.ssid == null || record.ssid !== group.ssid) return false;
	if (record.security_type == null) return true;
	const cls = catalogSecurityClass(record.security_type);
	return cls != null && groupClasses.has(cls);
}

type RelationshipBadge = "active" | "configured" | "intent" | null;

function groupBadge(
	group: ScanGroup,
	profiles: ConfiguredNetwork[],
	intent: DeviceNetworkIntent[],
): RelationshipBadge {
	if (group.ssid == null) return null;
	const classes = scanSecurityClasses(group.security);
	const matchingProfiles = profiles.filter((p) =>
		recordMatchesGroup(p, group, classes),
	);
	if (matchingProfiles.some((p) => p.is_active)) return "active";
	if (matchingProfiles.length > 0) return "configured";
	if (intent.some((i) => recordMatchesGroup(i, group, classes)))
		return "intent";
	return null;
}

function RelationshipBadgePill({ badge }: { badge: RelationshipBadge }) {
	if (badge === "active")
		return (
			<Badge variant="green" pill>
				Active
			</Badge>
		);
	if (badge === "configured")
		return (
			<Badge variant="blue" pill>
				Configured
			</Badge>
		);
	if (badge === "intent")
		return (
			<Badge variant="orange" pill>
				In intent
			</Badge>
		);
	return null;
}

interface NetworkCondition {
	profile_name: string;
	state: "Applied" | "Failed";
	reason: "WrongPSK" | "NotInRange" | "NmcliError" | "ActiveProfileKept" | null;
	message: string | null;
}

interface CatalogNetwork {
	id: number;
	name: string;
	ssid: string | null;
	network_type: string;
	is_network_hidden: boolean;
	password: string | null;
	description: string | null;
}

type SyncState = "unknown" | "pending" | "applying" | "error" | "synced";

function parseConditions(raw: unknown): NetworkCondition[] {
	if (!Array.isArray(raw)) return [];
	return raw as NetworkCondition[];
}

function deriveSyncState(
	intentVersion: number,
	observedVersion: number | undefined,
	conditions: NetworkCondition[],
	applying: boolean,
): SyncState {
	if (observedVersion == null) return "unknown";
	if (observedVersion < intentVersion) return applying ? "applying" : "pending";
	if (conditions.some((c) => c.state === "Failed")) return "error";
	return "synced";
}

// Tracks a dispatched command until the DB confirms the device responded (any
// row timestamp newer than the dispatch) or a 45s timeout, which covers empty
// result sets and offline devices. `syncing` lives in the caller because the
// query's refetchInterval needs it before the query provides `data`.
function useCommandSync<T>(
	syncing: boolean,
	setSyncing: (value: boolean) => void,
	data: T[] | undefined,
	getTimestamp: (item: T) => string,
) {
	const dispatchedAt = useRef<Date | null>(null);

	useEffect(() => {
		if (
			syncing &&
			dispatchedAt.current !== null &&
			data?.some((item) => new Date(getTimestamp(item)) > dispatchedAt.current!)
		) {
			setSyncing(false);
			dispatchedAt.current = null;
		}
	}, [data, syncing, setSyncing, getTimestamp]);

	useEffect(() => {
		if (!syncing) return;
		const timer = setTimeout(() => {
			setSyncing(false);
			dispatchedAt.current = null;
		}, 45_000);
		return () => clearTimeout(timer);
	}, [syncing, setSyncing]);

	return () => {
		setSyncing(true);
		dispatchedAt.current = new Date();
	};
}

function SyncChip({
	state,
	conditions,
}: {
	state: SyncState;
	conditions: NetworkCondition[];
}) {
	const [expanded, setExpanded] = useState(false);
	if (state === "unknown") return <Badge variant="gray">Unknown</Badge>;
	if (state === "pending") return <Badge variant="orange">Pending</Badge>;
	if (state === "applying")
		return (
			<span className="inline-flex items-center gap-1.5">
				<span className="relative flex h-2 w-2">
					<span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-yellow-400 opacity-75" />
					<span className="relative inline-flex rounded-full h-2 w-2 bg-yellow-500" />
				</span>
				<Badge variant="yellow">Applying...</Badge>
			</span>
		);
	if (state === "synced") return <Badge variant="green">Synced</Badge>;
	const failed = conditions.filter((c) => c.state === "Failed");
	return (
		<span className="inline-flex flex-col gap-1">
			<button
				type="button"
				onClick={() => setExpanded((p) => !p)}
				className="inline-flex items-center gap-1 cursor-pointer"
			>
				<Badge variant="red">Error</Badge>
				<ChevronDown
					className={`w-3 h-3 text-red-500 transition-transform ${expanded ? "rotate-180" : ""}`}
				/>
			</button>
			{expanded && failed.length > 0 && (
				<ul className="text-xs text-red-700 space-y-0.5 pl-1">
					{failed.map((c) => (
						<li key={c.profile_name}>
							<span className="font-mono">{c.profile_name}</span>
							{c.reason ? ` [${c.reason}]` : ""}
							{c.message ? `: ${c.message}` : ""}
						</li>
					))}
				</ul>
			)}
		</span>
	);
}

interface WifiPanelProps {
	serial: string;
	device: Device;
}

const WifiPanel = ({ serial, device }: WifiPanelProps) => {
	const [revealedIds, setRevealedIds] = useState<Set<string>>(new Set());
	const [syncing, setSyncing] = useState(false);
	const [scanSyncing, setScanSyncing] = useState(false);
	const [scanQuery, setScanQuery] = useState("");
	const [bandFilter, setBandFilter] = useState("all");
	const [securityFilter, setSecurityFilter] = useState("all");
	const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());

	function toggleGroup(key: string) {
		setExpandedGroups((prev) => {
			const next = new Set(prev);
			if (next.has(key)) {
				next.delete(key);
			} else {
				next.add(key);
			}
			return next;
		});
	}

	const {
		data: profilesRaw,
		isLoading,
		isError,
	} = useGetConfiguredNetworksForDevice(serial, {
		query: { refetchInterval: syncing ? 3000 : 30000 },
	});
	// Guard against a non-array body (e.g. an axios "success" resolved from a
	// terminated request with an empty response): `?? []`/`?.` only catch
	// null/undefined, so a stray "" or object would make .map/.filter/spread
	// below throw and surface react-router's error page for a frame.
	const profiles = Array.isArray(profilesRaw) ? profilesRaw : [];

	const currentNetwork = profiles.find((p) => p.is_active);

	const {
		data: scanResultsRaw,
		isLoading: isScanLoading,
		isError: isScanError,
	} = useGetWifiScanForDevice(serial, {
		query: { refetchInterval: scanSyncing ? 3000 : false },
	});
	const scanResults = Array.isArray(scanResultsRaw) ? scanResultsRaw : [];

	const startProfileSync = useCommandSync(
		syncing,
		setSyncing,
		profiles,
		getProfileTimestamp,
	);
	const startScanSync = useCommandSync(
		scanSyncing,
		setScanSyncing,
		scanResults,
		getScanTimestamp,
	);

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
		onSuccess: startProfileSync,
	});

	const { mutate: dispatchScan, isPending: isScanDispatching } = useMutation({
		mutationFn: () =>
			fetcher({
				url: `/devices/${serial}/commands`,
				method: "POST",
				data: [{ id: -7, command: "WifiScan", continue_on_error: false }],
			}),
		onSuccess: startScanSync,
	});

	const securityOptions = useMemo(
		() => [...new Set(scanResults.map((r) => r.security ?? "Open"))].sort(),
		[scanResults],
	);

	const scanGroups = useMemo(
		() => groupScanResults(scanResults),
		[scanResults],
	);

	// A group matches if any of its APs matches (FR-A8): filtering an SSID down
	// to a subset of its BSSIDs would misrepresent the group's own aggregates
	// (best signal, AP count, bands). But once a group is shown, expanding it
	// shouldn't dump every AP back in: `visibleAps` narrows the drill-down to
	// the APs that actually satisfy each active filter. The text query is the
	// one exception: it's also matched against the SSID name, and when that's
	// what matched, narrowing by BSSID text would hide every AP.
	const filteredGroups = useMemo(() => {
		const query = scanQuery.trim().toLowerCase();
		return scanGroups
			.filter((group) => {
				if (query) {
					const matches =
						group.ssid?.toLowerCase().includes(query) ||
						group.aps.some((ap) => ap.bssid.toLowerCase().includes(query));
					if (!matches) return false;
				}
				if (
					bandFilter !== "all" &&
					!group.aps.some((ap) => ap.band === bandFilter)
				) {
					return false;
				}
				if (
					securityFilter !== "all" &&
					!group.aps.some((ap) => (ap.security ?? "Open") === securityFilter)
				) {
					return false;
				}
				return true;
			})
			.map((group) => {
				const ssidMatchedQuery =
					query !== "" && (group.ssid?.toLowerCase().includes(query) ?? false);
				const visibleAps = group.aps.filter((ap) => {
					if (
						query &&
						!ssidMatchedQuery &&
						!ap.bssid.toLowerCase().includes(query)
					) {
						return false;
					}
					if (bandFilter !== "all" && ap.band !== bandFilter) return false;
					if (
						securityFilter !== "all" &&
						(ap.security ?? "Open") !== securityFilter
					) {
						return false;
					}
					return true;
				});
				return { ...group, visibleAps };
			});
	}, [scanGroups, scanQuery, bandFilter, securityFilter]);

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

	// Intent section
	const deviceId = String(device.id);
	const queryClient = useQueryClient();

	const prevSyncing = useRef(false);
	useEffect(() => {
		if (prevSyncing.current && !syncing) {
			queryClient.invalidateQueries({ queryKey: getGetNetworksQueryKey() });
		}
		prevSyncing.current = syncing;
	}, [syncing, queryClient]);

	const { data: intentListRaw } = useGetDeviceIntent(deviceId, {
		query: { refetchInterval: 30_000 },
	});
	const sortedIntent = useMemo(
		() =>
			(Array.isArray(intentListRaw) ? [...intentListRaw] : []).sort(
				(a, b) => a.priority - b.priority,
			),
		[intentListRaw],
	);

	const badgeByGroupKey = useMemo(() => {
		const map = new Map<string, RelationshipBadge>();
		for (const group of scanGroups) {
			const badge = groupBadge(group, profiles, sortedIntent);
			if (badge) map.set(group.key, badge);
		}
		return map;
	}, [scanGroups, profiles, sortedIntent]);

	const [applying, setApplying] = useState(false);
	const { data: polledDevice } = useGetDeviceInfo(serial, {
		query: { refetchInterval: applying ? 3000 : false },
	});
	const effectiveDevice = polledDevice ?? device;

	const conditions = useMemo(
		() => parseConditions(effectiveDevice.network_conditions),
		[effectiveDevice.network_conditions],
	);

	useEffect(() => {
		if (
			applying &&
			effectiveDevice.observed_intent_version != null &&
			effectiveDevice.observed_intent_version >= effectiveDevice.intent_version
		) {
			setApplying(false);
		}
	}, [
		applying,
		effectiveDevice.observed_intent_version,
		effectiveDevice.intent_version,
	]);

	useEffect(() => {
		if (!applying) return;
		const timer = setTimeout(() => setApplying(false), 60_000);
		return () => clearTimeout(timer);
	}, [applying]);

	const syncState = deriveSyncState(
		effectiveDevice.intent_version,
		effectiveDevice.observed_intent_version ?? undefined,
		conditions,
		applying,
	);

	const [showAddPicker, setShowAddPicker] = useState(false);
	const [modalView, setModalView] = useState<"list" | "create">("list");
	const [networkSearch, setNetworkSearch] = useState("");
	const [revealedNetworkIds, setRevealedNetworkIds] = useState<Set<number>>(
		new Set(),
	);
	const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);

	// Create-network form state
	const [newNetName, setNewNetName] = useState("");
	const [newNetSsid, setNewNetSsid] = useState("");
	const [newNetPassword, setNewNetPassword] = useState("");
	const [newNetHidden, setNewNetHidden] = useState(false);
	const [newNetDescription, setNewNetDescription] = useState("");
	const [showNewNetPassword, setShowNewNetPassword] = useState(false);
	const [addIntentError, setAddIntentError] = useState<string | null>(null);
	const [listAddError, setListAddError] = useState<string | null>(null);

	function closeModal() {
		setShowAddPicker(false);
		setModalView("list");
		setNetworkSearch("");
		setRevealedNetworkIds(new Set());
		setNewNetName("");
		setNewNetSsid("");
		setNewNetPassword("");
		setNewNetHidden(false);
		setNewNetDescription("");
		setShowNewNetPassword(false);
		setAddIntentError(null);
		setListAddError(null);
	}

	useEffect(() => {
		if (!showAddPicker) return;
		function onKeyDown(e: KeyboardEvent) {
			if (e.key !== "Escape") return;
			if (modalView === "create") {
				setModalView("list");
				setAddIntentError(null);
			} else {
				setShowAddPicker(false);
				setModalView("list");
				setNetworkSearch("");
				setRevealedNetworkIds(new Set());
				setNewNetName("");
				setNewNetSsid("");
				setNewNetPassword("");
				setNewNetHidden(false);
				setNewNetDescription("");
				setShowNewNetPassword(false);
				setAddIntentError(null);
				setListAddError(null);
			}
		}
		document.addEventListener("keydown", onKeyDown);
		return () => document.removeEventListener("keydown", onKeyDown);
	}, [showAddPicker, modalView]);

	const { mutateAsync: createIntentAsync } = useCreateDeviceIntent();

	const { mutate: createIntent } = useCreateDeviceIntent({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({
					queryKey: getGetDeviceIntentQueryKey(deviceId),
				});
				queryClient.invalidateQueries({
					queryKey: getGetDeviceInfoQueryKey(serial),
				});
				closeModal();
			},
			onError: (err) => {
				const status = isAxiosError(err) ? err.response?.status : undefined;
				setListAddError(
					status === 409
						? "A network with this name is already in the intent."
						: "Failed to add network to intent.",
				);
			},
		},
	});

	const networkCreator = useClientMutatorWithStatus<{
		id: number;
		name: string;
	}>();
	const [toast, setToast] = useState<ToastState | null>(null);
	const { mutateAsync: deleteNetworkByIdAsync } = useDeleteNetworkById();
	const { mutate: createNetwork, isPending: isCreatingNetwork } = useMutation({
		mutationFn: async (): Promise<{ matchedName: string | null }> => {
			const requestedName = newNetName.trim();
			if (sortedIntent.some((e) => e.name === newNetName.trim())) {
				throw Object.assign(new Error("intent-conflict"), { status: 409 });
			}
			const { data: created, status } = await networkCreator({
				url: "/networks",
				method: "POST",
				headers: { "Content-Type": "application/json" },
				data: {
					network_type: "wifi",
					is_network_hidden: newNetHidden,
					ssid: newNetSsid.trim() || null,
					name: newNetName.trim(),
					description: newNetDescription.trim() || null,
					password: newNetPassword || null,
				},
			});
			const maxPriority =
				sortedIntent.length > 0
					? Math.max(...sortedIntent.map((e) => e.priority))
					: 0;
			try {
				await createIntentAsync({
					deviceId,
					data: {
						network_id: created.id,
						priority: maxPriority + 1,
						managed_by: "operator",
					},
				});
			} catch (err) {
				// Only compensate when the POST actually created this row (201). A 200
				// means content-addressing matched an existing row that other devices
				// may already reference; deleting it would corrupt their intents.
				if (status === 201) {
					await deleteNetworkByIdAsync({ networkId: created.id }).catch(
						() => {},
					);
				}
				throw err;
			}
			// A 200 means the POST matched an existing catalog row by content, so
			// the name the operator typed was discarded in favour of that row's.
			// Silently showing a different name in the intent list looks like a bug.
			return {
				matchedName:
					status === 200 && created.name !== requestedName
						? created.name
						: null,
			};
		},
		onSuccess: ({ matchedName }) => {
			queryClient.invalidateQueries({ queryKey: getGetNetworksQueryKey() });
			queryClient.invalidateQueries({
				queryKey: getGetDeviceIntentQueryKey(deviceId),
			});
			queryClient.invalidateQueries({
				queryKey: getGetDeviceInfoQueryKey(serial),
			});
			if (matchedName) {
				setToast({
					type: "success",
					message: `Matched the existing network "${matchedName}", which has the same SSID and password.`,
				});
			} else {
				setToast(null);
			}
			closeModal();
		},
		onError: (err) => {
			const status = isAxiosError(err)
				? err.response?.status
				: (err as { status?: number }).status;
			setAddIntentError(
				status === 409
					? "A network with this name is already in the intent."
					: "Failed to add network to intent.",
			);
		},
	});

	const { mutate: deleteIntent } = useDeleteDeviceIntent({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({
					queryKey: getGetDeviceIntentQueryKey(deviceId),
				});
				queryClient.invalidateQueries({
					queryKey: getGetDeviceInfoQueryKey(serial),
				});
				setConfirmDeleteId(null);
			},
		},
	});

	const { mutateAsync: updateIntent } = useUpdateDeviceIntent({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({
					queryKey: getGetDeviceIntentQueryKey(deviceId),
				});
				queryClient.invalidateQueries({
					queryKey: getGetDeviceInfoQueryKey(serial),
				});
			},
		},
	});

	const { mutate: triggerApply, isPending: isApplyPending } =
		useApplyDeviceIntent({
			mutation: {
				onSuccess: () => setApplying(true),
			},
		});

	// useGetNetworks returns void in the generated client (missing utoipa response annotation); cast is safe at runtime
	const { data: catalogRaw } = useGetNetworks();
	const wifiCatalog = (
		Array.isArray(catalogRaw) ? (catalogRaw as CatalogNetwork[]) : []
	).filter((n) => n.network_type === "wifi" && n.ssid != null);

	async function swapPriority(indexA: number, indexB: number) {
		const a = sortedIntent[indexA];
		const b = sortedIntent[indexB];
		try {
			await updateIntent({
				deviceId,
				intentId: a.id,
				data: { priority: b.priority },
			});
			await updateIntent({
				deviceId,
				intentId: b.id,
				data: { priority: a.priority },
			});
		} catch {
			// Revert both sides to leave priorities consistent. Errors here are swallowed:
			// worst case the user sees a stale order until the next refetch.
			await updateIntent({
				deviceId,
				intentId: a.id,
				data: { priority: a.priority },
			}).catch(() => {});
			await updateIntent({
				deviceId,
				intentId: b.id,
				data: { priority: b.priority },
			}).catch(() => {});
		}
	}

	return (
		<Panel title="WiFi" icon={Wifi} theme={SECTION_THEMES.orange}>
			<Toast toast={toast} onClose={() => setToast(null)} />
			{/* Intent section */}
			<div className="mb-6 pb-6 border-b border-gray-100">
				<div className="flex items-center justify-between gap-2 mb-3">
					<div className="flex items-center gap-2">
						<p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
							Intent
						</p>
						<SyncChip state={syncState} conditions={conditions} />
					</div>
					<Button
						variant="solid"
						tone="orange"
						size="sm"
						disabled={sortedIntent.length === 0 || applying || isApplyPending}
						loading={applying || isApplyPending}
						onClick={() => triggerApply({ deviceId })}
					>
						Apply
					</Button>
				</div>

				{sortedIntent.length === 0 ? (
					<p className="text-sm text-gray-500 mb-3">
						No networks in intent. Add one below.
					</p>
				) : (
					<div className="divide-y divide-gray-100 mb-3">
						{sortedIntent.map((entry, idx) => (
							<div key={entry.id} className="py-2.5 flex items-center gap-2">
								<div className="flex flex-col">
									<button
										type="button"
										disabled={idx === 0}
										onClick={() => swapPriority(idx, idx - 1)}
										className="text-gray-400 hover:text-gray-600 disabled:opacity-30 disabled:cursor-not-allowed"
										aria-label="Move up"
									>
										<ChevronUp className="w-3.5 h-3.5" />
									</button>
									<button
										type="button"
										disabled={idx === sortedIntent.length - 1}
										onClick={() => swapPriority(idx, idx + 1)}
										className="text-gray-400 hover:text-gray-600 disabled:opacity-30 disabled:cursor-not-allowed"
										aria-label="Move down"
									>
										<ChevronDown className="w-3.5 h-3.5" />
									</button>
								</div>
								<span className="text-xs text-gray-400 w-4 text-center select-none">
									{idx + 1}
								</span>
								<div className="flex-1 min-w-0">
									<span className="text-sm font-medium text-gray-900">
										{entry.name}
									</span>
									{entry.ssid && (
										<span className="text-xs text-gray-500 font-mono ml-2">
											{entry.ssid}
										</span>
									)}
								</div>
								{confirmDeleteId === entry.id ? (
									<span className="inline-flex items-center gap-1.5 text-xs">
										<span className="text-red-600 font-medium">Remove?</span>
										<button
											type="button"
											onClick={() =>
												deleteIntent({ deviceId, intentId: entry.id })
											}
											className="text-red-600 hover:text-red-800 font-medium"
										>
											Confirm
										</button>
										<button
											type="button"
											onClick={() => setConfirmDeleteId(null)}
											className="text-gray-500 hover:text-gray-700"
										>
											Cancel
										</button>
									</span>
								) : (
									<button
										type="button"
										onClick={() => setConfirmDeleteId(entry.id)}
										className="text-gray-400 hover:text-red-500 transition-colors"
										aria-label={`Remove ${entry.name}`}
									>
										<Trash2 className="w-3.5 h-3.5" />
									</button>
								)}
							</div>
						))}
					</div>
				)}

				<Button
					variant="soft"
					tone="gray"
					size="sm"
					icon={<Plus className="w-3.5 h-3.5" />}
					onClick={() => setShowAddPicker(true)}
				>
					Add network
				</Button>

				{showAddPicker && (
					<div className="fixed inset-0 z-50 flex items-center justify-center">
						<div
							className="absolute inset-0 bg-black/40"
							onClick={closeModal}
						/>
						<div className="relative bg-white rounded-xl shadow-xl w-full max-w-4xl mx-4 max-h-[80vh] flex flex-col">
							<div className="flex items-center justify-between px-6 py-4 border-b border-gray-100">
								{modalView === "create" ? (
									<div className="flex items-center gap-2">
										<button
											type="button"
											onClick={() => setModalView("list")}
											className="text-gray-400 hover:text-gray-600 cursor-pointer"
											aria-label="Back to list"
										>
											<ArrowLeft className="w-4 h-4" />
										</button>
										<h2 className="text-sm font-semibold text-gray-900">
											New WiFi network
										</h2>
									</div>
								) : (
									<div className="flex items-center gap-3">
										<h2 className="text-sm font-semibold text-gray-900">
											Add network to intent
										</h2>
										<Button
											variant="soft"
											tone="blue"
											size="sm"
											icon={<Plus className="w-3.5 h-3.5" />}
											onClick={() => {
												setModalView("create");
												setListAddError(null);
											}}
										>
											New network
										</Button>
									</div>
								)}
								<button
									type="button"
									onClick={closeModal}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
									aria-label="Close"
								>
									<X className="w-4 h-4" />
								</button>
							</div>
							{modalView === "list" && (
								<div className="px-6 py-3 border-b border-gray-100">
									<SearchInput
										value={networkSearch}
										onChange={setNetworkSearch}
										placeholder="Search by name or SSID..."
									/>
								</div>
							)}
							{modalView === "create" ? (
								<>
									<div className="overflow-y-auto flex-1 p-6 space-y-4">
										<div>
											<label
												htmlFor="new-net-name"
												className="block text-xs font-medium text-gray-700 mb-1"
											>
												Name *
											</label>
											<input
												id="new-net-name"
												type="text"
												value={newNetName}
												onChange={(e) => setNewNetName(e.target.value)}
												className={`${filterFieldClass} w-full`}
												placeholder="e.g. Office WiFi"
											/>
										</div>
										<div>
											<label
												htmlFor="new-net-ssid"
												className="block text-xs font-medium text-gray-700 mb-1"
											>
												SSID *
											</label>
											<input
												id="new-net-ssid"
												type="text"
												value={newNetSsid}
												onChange={(e) => setNewNetSsid(e.target.value)}
												className={`${filterFieldClass} w-full`}
												placeholder="Network SSID"
											/>
										</div>
										<div>
											<label
												htmlFor="new-net-password"
												className="block text-xs font-medium text-gray-700 mb-1"
											>
												Password
											</label>
											<div className="relative">
												<input
													id="new-net-password"
													type={showNewNetPassword ? "text" : "password"}
													value={newNetPassword}
													onChange={(e) => setNewNetPassword(e.target.value)}
													className={`${filterFieldClass} w-full pr-10`}
													placeholder="Leave empty for open networks"
												/>
												<button
													type="button"
													onClick={() => setShowNewNetPassword((p) => !p)}
													className="absolute right-2.5 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
													aria-label={
														showNewNetPassword
															? "Hide password"
															: "Show password"
													}
												>
													{showNewNetPassword ? (
														<EyeOff className="w-4 h-4" />
													) : (
														<Eye className="w-4 h-4" />
													)}
												</button>
											</div>
										</div>
										<div>
											<label
												htmlFor="new-net-description"
												className="block text-xs font-medium text-gray-700 mb-1"
											>
												Description
											</label>
											<input
												id="new-net-description"
												type="text"
												value={newNetDescription}
												onChange={(e) => setNewNetDescription(e.target.value)}
												className={`${filterFieldClass} w-full`}
												placeholder="Optional"
											/>
										</div>
										<div className="flex items-center gap-2">
											<input
												id="new-net-hidden"
												type="checkbox"
												checked={newNetHidden}
												onChange={(e) => setNewNetHidden(e.target.checked)}
												className="rounded border-gray-300 text-blue-600"
											/>
											<label
												htmlFor="new-net-hidden"
												className="text-sm text-gray-700"
											>
												Hidden network (SSID not broadcast)
											</label>
										</div>
									</div>
									{addIntentError && (
										<p className="px-6 py-2 text-sm text-red-600">
											{addIntentError}
										</p>
									)}
									<div className="flex items-center justify-end gap-2 px-6 py-4 border-t border-gray-100">
										<Button
											variant="ghost"
											tone="gray"
											size="sm"
											onClick={() => {
												setModalView("list");
												setAddIntentError(null);
											}}
										>
											Cancel
										</Button>
										<Button
											variant="solid"
											tone="blue"
											size="sm"
											disabled={
												!newNetName.trim() ||
												!newNetSsid.trim() ||
												isCreatingNetwork
											}
											loading={isCreatingNetwork}
											onClick={() => createNetwork()}
										>
											Create and add to intent
										</Button>
									</div>
								</>
							) : (
								<div className="overflow-y-auto flex-1">
									{listAddError && (
										<p className="px-6 pt-4 text-sm text-red-600">
											{listAddError}
										</p>
									)}
									{(() => {
										const q = networkSearch.trim().toLowerCase();
										const filtered = q
											? wifiCatalog.filter(
													(n) =>
														n.name.toLowerCase().includes(q) ||
														(n.ssid?.toLowerCase().includes(q) ?? false),
												)
											: wifiCatalog;
										return filtered.length === 0 ? (
											<p className="text-sm text-gray-500 p-6">
												{q
													? "No networks match your search."
													: "No WiFi networks in catalog."}
											</p>
										) : (
											<table className="min-w-full divide-y divide-gray-200">
												<thead className="sticky top-0 bg-white">
													<tr>
														{[
															"Name",
															"SSID",
															"Password",
															"Type",
															"Hidden",
															"",
														].map((col) => (
															<th
																key={col}
																scope="col"
																className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
															>
																{col}
															</th>
														))}
													</tr>
												</thead>
												<tbody className="divide-y divide-gray-100">
													{filtered.map((n) => {
														const revealed = revealedNetworkIds.has(n.id);
														return (
															<tr
																key={n.id}
																className="hover:bg-gray-50 transition-colors"
															>
																<td className="px-4 py-3 text-sm font-medium text-gray-900">
																	{n.name}
																	{n.description && (
																		<p className="text-xs text-gray-400 font-normal">
																			{n.description}
																		</p>
																	)}
																</td>
																<td className="px-4 py-3 text-sm font-mono text-gray-600">
																	{n.ssid ?? (
																		<span className="text-gray-400 italic">
																			—
																		</span>
																	)}
																</td>
																<td className="px-4 py-3 min-w-[16rem]">
																	{n.password ? (
																		<div className="flex items-center gap-1.5">
																			<span className="text-sm font-mono text-gray-700">
																				{revealed ? n.password : MASK}
																			</span>
																			<button
																				type="button"
																				onClick={() =>
																					setRevealedNetworkIds((prev) => {
																						const next = new Set(prev);
																						next.has(n.id)
																							? next.delete(n.id)
																							: next.add(n.id);
																						return next;
																					})
																				}
																				className="text-gray-400 hover:text-gray-600"
																				aria-label={
																					revealed
																						? "Hide password"
																						: "Reveal password"
																				}
																			>
																				{revealed ? (
																					<EyeOff className="w-3.5 h-3.5" />
																				) : (
																					<Eye className="w-3.5 h-3.5" />
																				)}
																			</button>
																		</div>
																	) : (
																		<span className="text-xs text-gray-400">
																			None
																		</span>
																	)}
																</td>
																<td className="px-4 py-3">
																	<Badge variant="gray">{n.network_type}</Badge>
																</td>
																<td className="px-4 py-3 text-center">
																	{n.is_network_hidden ? (
																		<span className="text-green-600 text-xs font-medium">
																			Yes
																		</span>
																	) : (
																		<span className="text-gray-400 text-xs">
																			No
																		</span>
																	)}
																</td>
																<td className="px-4 py-3 text-right">
																	<Button
																		variant="soft"
																		tone="blue"
																		size="sm"
																		onClick={() => {
																			const maxPriority =
																				sortedIntent.length > 0
																					? Math.max(
																							...sortedIntent.map(
																								(e) => e.priority,
																							),
																						)
																					: 0;
																			createIntent({
																				deviceId,
																				data: {
																					network_id: n.id,
																					priority: maxPriority + 1,
																					managed_by: "operator",
																				},
																			});
																		}}
																	>
																		Add
																	</Button>
																</td>
															</tr>
														);
													})}
												</tbody>
											</table>
										);
									})()}
								</div>
							)}
						</div>
					</div>
				)}
			</div>

			{/* Splits at xl, not lg: the panel itself only takes two thirds of the
			    Network tab, so the scan table needs the extra width. */}
			<div className="grid grid-cols-1 xl:grid-cols-2 gap-6">
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
				<div className="border-t border-gray-100 pt-4 xl:border-t-0 xl:pt-0 xl:border-l xl:border-gray-100 xl:pl-6">
					<div className="flex items-center justify-between gap-2 mb-3">
						{isScanError ? (
							<p className="text-sm text-red-500">
								Failed to load WiFi scan results.
							</p>
						) : (
							<p className="text-sm text-gray-500">
								{/* All rows share one scanned_at: the API replaces them atomically. */}
								{scanResults && scanResults.length > 0
									? `Last checked ${new Date(scanResults[0].scanned_at).toLocaleString()}`
									: "Never scanned"}
							</p>
						)}
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
							{filteredGroups.length === 0 ? (
								<p className="text-sm text-gray-500">
									No networks match the filters.
								</p>
							) : (
								<div className="overflow-x-auto">
									<table className="min-w-full divide-y divide-gray-200">
										<thead>
											<tr>
												{[
													{
														label: "Network",
														className: "py-2 pr-2 text-left",
													},
													{
														label: "Signal",
														className: "px-2 py-2 text-center",
													},
													{ label: "APs", className: "px-2 py-2 text-right" },
													{ label: "Bands", className: "px-2 py-2 text-left" },
													{
														label: "Security",
														className: "pl-2 py-2 text-left",
													},
												].map((col) => (
													<th
														key={col.label}
														scope="col"
														className={`${col.className} text-xs font-medium text-gray-500 uppercase tracking-wider`}
													>
														{col.label}
													</th>
												))}
											</tr>
										</thead>
										<tbody className="divide-y divide-gray-100">
											{filteredGroups.map((group) => {
												const expanded = expandedGroups.has(group.key);
												const hidden = group.ssid == null;
												const badge = badgeByGroupKey.get(group.key) ?? null;
												return (
													<Fragment key={group.key}>
														<tr>
															<td className="py-2 pr-2 max-w-0 w-full">
																<button
																	type="button"
																	onClick={() => toggleGroup(group.key)}
																	aria-expanded={expanded}
																	aria-label={`${expanded ? "Collapse" : "Expand"} ${
																		hidden ? "hidden networks" : group.ssid
																	}`}
																	className="flex items-center gap-1.5 min-w-0 w-full text-left cursor-pointer"
																>
																	{expanded ? (
																		<ChevronDown className="w-3.5 h-3.5 text-gray-400 flex-shrink-0" />
																	) : (
																		<ChevronRight className="w-3.5 h-3.5 text-gray-400 flex-shrink-0" />
																	)}
																	<span className="flex flex-col min-w-0">
																		{hidden ? (
																			<span className="text-sm italic text-gray-500 truncate">
																				{group.aps.length} hidden AP
																				{group.aps.length === 1 ? "" : "s"} seen
																			</span>
																		) : (
																			<span className="flex items-center gap-1.5 min-w-0">
																				<span className="text-sm font-medium text-gray-900 truncate">
																					{group.ssid}
																				</span>
																				<RelationshipBadgePill badge={badge} />
																			</span>
																		)}
																	</span>
																</button>
															</td>
															<td className="px-2 py-2 text-center whitespace-nowrap">
																<SignalBar value={group.bestSignal} />
															</td>
															<td className="px-2 py-2 text-xs text-gray-500 text-right whitespace-nowrap tabular-nums">
																×{group.aps.length}
															</td>
															<td className="px-2 py-2 whitespace-nowrap">
																<span className="inline-flex gap-1">
																	{group.bands.map((band) => (
																		<Badge key={band} variant="gray">
																			{band}
																		</Badge>
																	))}
																</span>
															</td>
															<td className="pl-2 py-2 whitespace-nowrap">
																{!hidden && (
																	<Badge variant="gray" pill>
																		{group.security ?? "Open"}
																	</Badge>
																)}
															</td>
														</tr>
														{expanded &&
															group.visibleAps.map((ap: WifiScanResult) => (
																<tr
																	key={`${ap.bssid}-${ap.channel}`}
																	className="bg-gray-50"
																>
																	<td className="py-1.5 pr-2 pl-8 text-xs font-mono text-gray-500 whitespace-nowrap">
																		{ap.bssid}
																	</td>
																	<td className="px-2 py-1.5 text-center whitespace-nowrap">
																		<SignalBar value={ap.signal ?? null} />
																	</td>
																	<td />
																	<td className="px-2 py-1.5 text-xs text-gray-500 whitespace-nowrap">
																		<Tooltip
																			content={`Channel ${ap.channel ?? "—"}`}
																		>
																			<span className="border-b border-dotted border-gray-500 cursor-default">
																				{ap.band ?? "—"}
																			</span>
																		</Tooltip>
																	</td>
																	<td className="pl-2 py-1.5 text-right text-xs text-gray-500 tabular-nums whitespace-nowrap">
																		{ap.rate != null ? `${ap.rate} Mbps` : "—"}
																	</td>
																</tr>
															))}
													</Fragment>
												);
											})}
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
