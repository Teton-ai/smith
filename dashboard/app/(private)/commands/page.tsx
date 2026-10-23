import { useAuth0 } from "@auth0/auth0-react";
import { Badge, Button, Card, Select } from "@teton/smith-ui";
import { AlertCircle, Loader2, Send } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
	type BundleWithCommands,
	type BundleWithCommandsPaginated,
	type DeviceCommandResponse,
	useGetBundle,
	useGetBundleCommandsInfinite,
	useGetUsers,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import {
	SelectionMessage,
	selectionSearch,
	useRevealSelection,
	validBundleId,
} from "./selection";
import {
	CommandBody,
	CommandHeader,
	CommandListRow,
	CommandMeta,
	getCommandStatus,
	getTxLabel,
} from "./shared";

const PAGE_SIZE = 50;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const getBundleStats = (responses: DeviceCommandResponse[]) => {
	const stats = {
		total: responses.length,
		success: 0,
		failed: 0,
		pending: 0,
		executing: 0,
		cancelled: 0,
	};
	for (const response of responses) {
		const status = getCommandStatus(response);
		if (status === "success") stats.success++;
		else if (status === "failed") stats.failed++;
		else if (status === "pending") stats.pending++;
		else if (status === "executing") stats.executing++;
		else if (status === "cancelled") stats.cancelled++;
	}
	return stats;
};

// ---------------------------------------------------------------------------
// Single command result (TX + response) for one device
// ---------------------------------------------------------------------------

const CommandResult = ({ response }: { response: DeviceCommandResponse }) => {
	const [showRaw, setShowRaw] = useState(false);

	return (
		<div className="border-b border-gray-100 last:border-b-0">
			<CommandHeader
				cmd={response}
				showRaw={showRaw}
				onToggleRaw={() => setShowRaw((v) => !v)}
			/>
			<div className="px-5 py-3">
				<CommandBody cmd={response} showRaw={showRaw} />
			</div>
		</div>
	);
};

// ---------------------------------------------------------------------------
// Bundle detail (right panel): device list + that device's commands in order
// ---------------------------------------------------------------------------

const BundleDetail = ({ bundle }: { bundle: BundleWithCommands }) => {
	// A bundle can contain multiple commands per device (e.g. a recipe), so
	// group responses by device and keep each device's commands in issue order.
	const devices = useMemo(() => {
		const byDevice = new Map<
			number,
			{ device: number; serial: string; commands: DeviceCommandResponse[] }
		>();
		for (const response of bundle.responses) {
			const entry = byDevice.get(response.device) ?? {
				device: response.device,
				serial: response.serial_number,
				commands: [],
			};
			entry.commands.push(response);
			byDevice.set(response.device, entry);
		}
		const list = Array.from(byDevice.values());
		for (const entry of list) {
			entry.commands.sort((a, b) => a.cmd_id - b.cmd_id);
		}
		return list;
	}, [bundle]);

	const [searchParams, setSearchParams] = useSearchParams();
	const deviceSerial = searchParams.get("device");
	const selected = devices.find((d) => d.serial === deviceSerial) ?? null;
	useEffect(() => {
		if (deviceSerial === null && devices[0]) {
			setSearchParams(
				selectionSearch(searchParams, { device: devices[0].serial }),
				{ replace: true },
			);
		}
	}, [deviceSerial, devices, searchParams, setSearchParams]);

	return (
		<div className="flex h-full overflow-hidden">
			{/* Device list */}
			<div className="w-2/5 border-r border-gray-200 overflow-y-auto overflow-x-hidden shrink-0">
				{devices.map((d) => {
					const stats = getBundleStats(d.commands);
					const isSelected = d.serial === deviceSerial;
					return (
						<Link
							key={d.device}
							to={selectionSearch(searchParams, { device: d.serial })}
							className={`block w-full text-left px-4 py-3 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
								isSelected
									? "bg-blue-50 border-l-2 border-l-blue-500"
									: "hover:bg-gray-50 border-l-2 border-l-transparent"
							}`}
						>
							<div className="flex items-center justify-between gap-2 mb-1 min-w-0">
								<span
									className={`text-sm font-mono truncate min-w-0 ${isSelected ? "text-blue-900" : "text-gray-900"}`}
								>
									{d.serial}
								</span>
								<span className="text-xs text-gray-400 shrink-0">
									{d.commands.length} {d.commands.length === 1 ? "cmd" : "cmds"}
								</span>
							</div>
							<div className="flex items-center gap-1.5 flex-wrap">
								{stats.success > 0 && (
									<Badge variant="green" pill>
										{stats.success} ok
									</Badge>
								)}
								{stats.failed > 0 && (
									<Badge variant="red" pill>
										{stats.failed} failed
									</Badge>
								)}
								{stats.pending > 0 && (
									<Badge variant="yellow" pill>
										{stats.pending} pending
									</Badge>
								)}
								{stats.executing > 0 && (
									<Badge variant="blue" pill>
										{stats.executing} executing
									</Badge>
								)}
							</div>
						</Link>
					);
				})}
			</div>

			{/* Selected device's commands, in order */}
			<div className="flex-1 flex flex-col overflow-hidden">
				{selected != null ? (
					<>
						<div className="px-5 py-3 border-b border-gray-200 shrink-0">
							<Link
								to={`/devices/${selected.serial}/commands`}
								className="text-sm font-mono font-medium text-blue-600 hover:underline"
							>
								{selected.serial}
							</Link>
							<CommandMeta trigger={bundle.user_email} bundleId={bundle.uuid} />
						</div>
						<div className="flex-1 overflow-y-auto overflow-x-hidden">
							{selected.commands.map((c) => (
								<CommandResult key={c.cmd_id} response={c} />
							))}
						</div>
					</>
				) : (
					<div className="flex items-center justify-center h-full text-gray-400 text-sm">
						{deviceSerial !== null
							? "Device not found in this bundle"
							: "Select a device to see its responses"}
					</div>
				)}
			</div>
		</div>
	);
};

// ---------------------------------------------------------------------------
// Triggered-by filter
// ---------------------------------------------------------------------------

// The presets (All, People, System, Me) are remembered across visits;
// an individual user is only ever in the URL.
const PRESET_KEY = "commands:triggered-by";
const NAMED_PRESETS = ["people", "system", "me"];

const readPreset = (): string => {
	try {
		const saved = localStorage.getItem(PRESET_KEY);
		return saved != null && NAMED_PRESETS.includes(saved) ? saved : "";
	} catch {
		return "";
	}
};

const writePreset = (preset: string) => {
	try {
		if (preset) localStorage.setItem(PRESET_KEY, preset);
		else localStorage.removeItem(PRESET_KEY);
	} catch {
		// Storage can be unavailable (private mode); the choice just won't persist.
	}
};

// `?by=` always wins over the saved preset and never writes it, so a shared
// link shows what it says without changing the viewer's preference.
const useTriggeredByFilter = () => {
	const [searchParams, setSearchParams] = useSearchParams();
	const [savedPreset, setSavedPreset] = useState(readPreset);
	const byParam = searchParams.get("by");

	// Mirror a restored preset into the URL so a copied link is explicit.
	useEffect(() => {
		if (byParam == null && savedPreset && !searchParams.has("id")) {
			setSearchParams(
				(prev) => {
					const next = new URLSearchParams(prev);
					next.set("by", savedPreset);
					return next;
				},
				{ replace: true },
			);
		}
	}, [byParam, savedPreset, searchParams, setSearchParams]);

	const setValue = (value: string) => {
		// Saved before the URL changes: with `by` gone, the old preset would
		// otherwise be restored over an explicit "All".
		if (value === "" || NAMED_PRESETS.includes(value)) {
			writePreset(value);
			setSavedPreset(value);
		}
		const next = new URLSearchParams(searchParams);
		next.delete("id");
		next.delete("device");
		if (value) next.set("by", value);
		else next.delete("by");
		setSearchParams(next);
	};

	// null = All.
	return {
		value: byParam ?? (searchParams.has("id") ? null : savedPreset || null),
		setValue,
	};
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

const CommandsPage = () => {
	const { user } = useAuth0();
	const fetcher = useClientMutator<BundleWithCommandsPaginated>();

	// null = All. Otherwise "people", "system", "me", or a user id.
	const { value: triggeredBy, setValue: setFilter } = useTriggeredByFilter();

	// Listing users needs `users:read`, which reading commands does not, so this
	// is allowed to fail: the per-person options just disappear.
	const { data: users } = useGetUsers();

	const {
		data: bundleData,
		isLoading,
		isError,
		fetchNextPage,
		hasNextPage,
		isFetchingNextPage,
		isFetching,
		isFetchNextPageError,
	} = useGetBundleCommandsInfinite({
		query: {
			// The generated key is static, so it has to carry the filter or React
			// Query serves the previous filter's pages.
			queryKey: ["/commands/bundles", triggeredBy],
			initialPageParam: undefined as string | undefined,
			getNextPageParam: (lastPage) => {
				if (!lastPage?.next) return undefined;
				// next is a full URL like: https://.../commands/bundles?starting_after={uuid}&limit=100
				const url = new URL(lastPage.next);
				return url.searchParams.get("starting_after") ?? undefined;
			},
			queryFn: ({ signal, pageParam }) =>
				fetcher({
					url: "/commands/bundles",
					method: "GET",
					params: {
						limit: PAGE_SIZE,
						...(pageParam ? { starting_after: pageParam } : {}),
						...(triggeredBy ? { triggered_by: triggeredBy } : {}),
					},
					signal,
				}),
			refetchInterval: 5000,
		},
	});

	const [searchParams, setSearchParams] = useSearchParams();
	const id = searchParams.get("id");
	const validId = validBundleId(id);
	const selectedUuid = validId ? id.toLowerCase() : null;
	const detail = useGetBundle(selectedUuid ?? "", {
		query: { enabled: validId, refetchInterval: 5000 },
	});
	const scrollRef = useRef<HTMLDivElement>(null);

	const handleScroll = useCallback(() => {
		const el = scrollRef.current;
		if (!el || !hasNextPage || isFetchingNextPage) return;
		if (el.scrollHeight - el.scrollTop - el.clientHeight < 600) {
			fetchNextPage();
		}
	}, [hasNextPage, isFetchingNextPage, fetchNextPage]);

	const bundles = useMemo(() => {
		const all = (bundleData?.pages ?? []).flatMap((p) => p?.bundles ?? []);
		const seen = new Set<string>();
		return all.filter((b) => {
			if (seen.has(b.uuid)) return false;
			seen.add(b.uuid);
			return true;
		});
	}, [bundleData]);

	useEffect(() => {
		if (bundles.length > 0 && id === null) {
			setSearchParams(
				selectionSearch(searchParams, {
					id: bundles[0].uuid,
					by: triggeredBy,
					device: bundles[0].responses[0]?.serial_number ?? null,
				}),
				{ replace: true },
			);
		}
	}, [bundles, id, triggeredBy, searchParams, setSearchParams]);

	const selectedBundle =
		detail.data ?? bundles.find((b) => b.uuid === selectedUuid) ?? null;
	const filterEmail =
		triggeredBy === "me"
			? user?.email
			: users?.find((person) => String(person.id) === triggeredBy)?.email;
	// An individual user's filter still works without users:read. When its
	// email cannot be resolved, let the filtered history locate the bundle.
	const unresolvedUserFilter =
		!!triggeredBy &&
		!NAMED_PRESETS.includes(triggeredBy) &&
		filterEmail == null;
	const selectionMatchesFilter =
		!triggeredBy ||
		(triggeredBy === "people" && !!selectedBundle?.user_email) ||
		(triggeredBy === "system" && selectedBundle?.user_email == null) ||
		unresolvedUserFilter ||
		(filterEmail != null && selectedBundle?.user_email === filterEmail);
	useRevealSelection({
		selectionKey:
			selectedUuid === null ? null : `${triggeredBy}:${selectedUuid}`,
		containerRef: scrollRef,
		enabled: selectedBundle !== null && selectionMatchesFilter,
		hasNextPage,
		isFetching,
		isFetchNextPageError,
		fetchNextPage,
	});

	// Accounts without an email would render as blank rows, so drop them.
	const userOptions = useMemo(
		() =>
			(users ?? [])
				.flatMap((user) => {
					const email = user.email?.trim();
					return email ? [{ id: user.id, email }] : [];
				})
				.sort((a, b) => a.email.localeCompare(b.email)),
		[users],
	);

	// A hand-edited or stale `?by=` would otherwise match no option and display
	// as "All" while the list stays filtered.
	const unknownFilter =
		triggeredBy != null &&
		triggeredBy !== "people" &&
		triggeredBy !== "system" &&
		triggeredBy !== "me" &&
		!userOptions.some((user) => String(user.id) === triggeredBy)
			? triggeredBy
			: null;

	const filterBar = (
		<div className="flex items-center gap-3 mb-4 shrink-0">
			<label htmlFor="triggered-by" className="text-sm text-gray-500">
				Triggered by
			</label>
			<Select id="triggered-by" value={triggeredBy ?? ""} onChange={setFilter}>
				<option value="">All</option>
				<option value="people">People</option>
				<option value="system">System</option>
				<option value="me">Me</option>
				{userOptions.length > 0 && (
					<optgroup label="Individual users">
						{userOptions.map((user) => (
							<option key={user.id} value={String(user.id)}>
								{user.email}
							</option>
						))}
					</optgroup>
				)}
				{unknownFilter && (
					<option value={unknownFilter}>Unknown user ({unknownFilter})</option>
				)}
			</Select>
		</div>
	);

	const wrap = (children: React.ReactNode) => (
		<div className="flex-1 overflow-hidden p-4 sm:p-6 lg:p-8 flex flex-col">
			{filterBar}
			{children}
		</div>
	);

	if (isLoading && id === null) {
		return wrap(
			<div className="flex items-center justify-center py-12">
				<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
			</div>,
		);
	}

	// A failed background refetch also sets `isError`, so gate on having nothing
	// to show: never blank a rendered list because one 5s poll blipped.
	if (isError && bundles.length === 0 && id === null) {
		return wrap(
			<Card className="text-center py-12">
				<AlertCircle className="w-12 h-12 text-red-300 mx-auto mb-3" />
				<p className="text-gray-500">Failed to load commands</p>
				<p className="text-sm text-gray-400 mt-1">
					The request did not complete. It will retry automatically.
				</p>
			</Card>,
		);
	}

	if (bundles.length === 0 && id === null) {
		return wrap(
			<Card className="text-center py-12">
				<Send className="w-12 h-12 text-gray-300 mx-auto mb-3" />
				{triggeredBy ? (
					<>
						<p className="text-gray-500">No commands match this filter</p>
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							className="mt-3"
							onClick={() => setFilter("")}
						>
							Clear filter
						</Button>
					</>
				) : (
					<>
						<p className="text-gray-500">
							No bulk commands have been executed yet
						</p>
						<p className="text-sm text-gray-400 mt-1">
							Select devices and run a command to see results here
						</p>
					</>
				)}
			</Card>,
		);
	}

	return wrap(
		<Card className="flex-1 overflow-hidden flex">
			{/* Left: bundle list (1/3) */}
			<div className="w-1/5 border-r border-gray-200 shrink-0 flex flex-col overflow-hidden">
				<div
					ref={scrollRef}
					onScroll={handleScroll}
					className="flex-1 overflow-y-auto overflow-x-hidden divide-y divide-gray-100"
				>
					{bundles.map((bundle) => {
						const stats = getBundleStats(bundle.responses);
						const firstCommand = bundle.responses[0];
						const { label: commandLabel } = firstCommand
							? getTxLabel(firstCommand.cmd_data)
							: { label: "Unknown Command" };

						return (
							<CommandListRow
								key={bundle.uuid}
								selected={bundle.uuid === selectedUuid}
								to={selectionSearch(searchParams, {
									id: bundle.uuid,
									device: bundle.responses[0]?.serial_number ?? null,
								})}
								title={commandLabel}
								trigger={bundle.user_email}
								time={bundle.created_on}
								stats={
									<>
										{stats.success > 0 && (
											<Badge variant="green" pill>
												{stats.success} ok
											</Badge>
										)}
										{stats.failed > 0 && (
											<Badge variant="red" pill>
												{stats.failed} failed
											</Badge>
										)}
										{stats.pending > 0 && (
											<Badge variant="yellow" pill>
												{stats.pending} pending
											</Badge>
										)}
										{stats.executing > 0 && (
											<Badge variant="blue" pill>
												{stats.executing} executing
											</Badge>
										)}
									</>
								}
							/>
						);
					})}
				</div>
				{isFetchNextPageError && (
					<Button variant="soft" tone="gray" onClick={() => fetchNextPage()}>
						Retry loading history
					</Button>
				)}
				{isFetchingNextPage && (
					<div className="flex items-center justify-center py-3 border-t border-gray-200 shrink-0">
						<Loader2 className="w-4 h-4 animate-spin text-gray-400" />
					</div>
				)}
			</div>

			{/* Right: bundle detail (2/3) */}
			<div className="w-4/5 overflow-hidden">
				{selectedBundle != null ? (
					<BundleDetail bundle={selectedBundle} />
				) : (
					<SelectionMessage
						invalid={id !== null && !validId}
						loading={validId && detail.isPending}
						error={detail.error}
						kind="bundle"
					/>
				)}
			</div>
		</Card>,
	);
};

export default CommandsPage;
