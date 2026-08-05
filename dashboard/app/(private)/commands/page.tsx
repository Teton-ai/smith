import {
	Badge,
	type BadgeVariant,
	Button,
	Card,
	Select,
} from "@teton/smith-ui";
import { AlertCircle, Loader2, Send, Terminal } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import {
	type BundleWithCommands,
	type BundleWithCommandsPaginated,
	type DeviceCommandResponse,
	useGetBundleCommandsInfinite,
	useGetUsers,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { RelativeTime } from "@/app/components/RelativeTime";
import {
	CodeBlock,
	getCommandStatus,
	getTxLabel,
	renderRxDetail,
	renderTxDetail,
} from "./shared";

const PAGE_SIZE = 50;

const STATUS_VARIANT: Record<string, BadgeVariant> = {
	success: "green",
	failed: "red",
	executing: "blue",
	cancelled: "gray",
	pending: "yellow",
};

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
	const status = getCommandStatus(response);
	const { label, mono } = getTxLabel(response.cmd_data);

	return (
		<div className="border-b border-gray-100 last:border-b-0">
			<div className="flex items-center justify-between gap-3 px-5 py-3 bg-gray-50/70">
				<div className="flex items-center gap-2 flex-wrap min-w-0">
					<span
						className={`text-sm font-semibold text-gray-900 truncate ${mono ? "font-mono" : ""}`}
					>
						{label}
					</span>
					<Badge variant={STATUS_VARIANT[status] ?? "gray"}>{status}</Badge>
					{response.response != null && response.status != null && (
						<Badge
							variant={response.status === 0 ? "green" : "red"}
							className="font-mono"
						>
							exit {response.status}
						</Badge>
					)}
				</div>
				{response.response != null && (
					<Button
						variant="soft"
						tone="gray"
						size="sm"
						className="shrink-0"
						onClick={() => setShowRaw((v) => !v)}
					>
						{showRaw ? "Formatted" : "Raw JSON"}
					</Button>
				)}
			</div>
			<div className="px-5 py-3">
				{showRaw ? (
					<>
						<CodeBlock
							label="raw TX"
							meta={
								<>
									· Issued <RelativeTime date={response.issued_at} />
								</>
							}
							content={JSON.stringify(response.cmd_data, null, 2)}
						/>
						<div className="mt-4">
							<CodeBlock
								label="raw RX"
								meta={
									response.response_at ? (
										<>
											· Responded <RelativeTime date={response.response_at} />
										</>
									) : (
										<span className="text-yellow-500">
											· Waiting for response…
										</span>
									)
								}
								content={JSON.stringify(response.response, null, 2)}
							/>
						</div>
					</>
				) : (
					<>
						<div className="mb-4">
							<div className="flex items-center gap-2 mb-3">
								<p className="text-xs font-medium uppercase tracking-wide text-gray-400">
									Sent
								</p>
								<span className="text-xs text-gray-400">
									· Issued <RelativeTime date={response.issued_at} />
								</span>
							</div>
							{renderTxDetail(response.cmd_data)}
						</div>
						<div className="flex items-center gap-2 mb-3">
							<p className="text-xs font-medium uppercase tracking-wide text-gray-400">
								Response
							</p>
							{response.response_at ? (
								<span className="text-xs text-gray-400">
									· Responded <RelativeTime date={response.response_at} />
								</span>
							) : (
								<span className="text-xs text-yellow-500">
									· Waiting for response…
								</span>
							)}
						</div>
						{renderRxDetail(response.response)}
					</>
				)}
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

	const [selectedDevice, setSelectedDevice] = useState<number>(
		devices[0]?.device ?? -1,
	);

	// If the selected device isn't in this bundle (e.g. bundle just changed),
	// sync to the first device immediately to avoid a stale highlight.
	const firstDevice = devices[0]?.device ?? -1;
	const deviceInBundle = devices.find((d) => d.device === selectedDevice);
	if (!deviceInBundle && selectedDevice !== firstDevice) {
		setSelectedDevice(firstDevice);
	}
	const selected = deviceInBundle ?? devices[0] ?? null;

	return (
		<div className="flex h-full overflow-hidden">
			{/* Device list */}
			<div className="w-2/5 border-r border-gray-200 overflow-y-auto overflow-x-hidden shrink-0">
				{devices.map((d) => {
					const stats = getBundleStats(d.commands);
					const isSelected = d.device === selectedDevice;
					return (
						<button
							key={d.device}
							type="button"
							onClick={() => setSelectedDevice(d.device)}
							className={`w-full text-left px-4 py-3 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
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
						</button>
					);
				})}
			</div>

			{/* Selected device's commands, in order */}
			<div className="flex-1 flex flex-col overflow-hidden">
				{selected != null ? (
					<>
						<div className="px-5 py-3 border-b border-gray-200 shrink-0">
							<div className="flex items-center justify-between gap-3">
								<Link
									to={`/devices/${selected.serial}/commands`}
									className="text-sm font-mono font-medium text-blue-600 hover:underline"
								>
									{selected.serial}
								</Link>
								<p
									className="text-xs text-gray-500 min-w-0 truncate"
									title={bundle.user_email ?? "System"}
								>
									Triggered by: {bundle.user_email ?? "System"}
								</p>
							</div>
						</div>
						<div className="flex-1 overflow-y-auto overflow-x-hidden">
							{selected.commands.map((c) => (
								<CommandResult key={c.cmd_id} response={c} />
							))}
						</div>
					</>
				) : (
					<div className="flex items-center justify-center h-full text-gray-400 text-sm">
						Select a device to see its responses
					</div>
				)}
			</div>
		</div>
	);
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

const CommandsPage = () => {
	const fetcher = useClientMutator<BundleWithCommandsPaginated>();
	const [searchParams, setSearchParams] = useSearchParams();

	// null = All. Otherwise "people", "system", or a user id.
	const triggeredBy = searchParams.get("by");

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

	const [selectedUuid, setSelectedUuid] = useState<string | null>(null);
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

	// The previous filter's selection is meaningless once the list changes.
	// Adjusted during render, not in an effect, so it covers every path the
	// filter can change by (select, back/forward, pasted URL).
	const [filterOfSelection, setFilterOfSelection] = useState(triggeredBy);
	if (filterOfSelection !== triggeredBy) {
		setFilterOfSelection(triggeredBy);
		setSelectedUuid(null);
	}

	// Auto-select first bundle
	useEffect(() => {
		if (bundles.length > 0 && selectedUuid === null) {
			setSelectedUuid(bundles[0].uuid);
		}
	}, [bundles, selectedUuid]);

	const selectedBundle = bundles.find((b) => b.uuid === selectedUuid) ?? null;

	const setFilter = (value: string) => {
		const next = new URLSearchParams(searchParams);
		if (value) next.set("by", value);
		else next.delete("by");
		setSearchParams(next);
	};

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

	if (isLoading) {
		return wrap(
			<div className="flex items-center justify-center py-12">
				<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
			</div>,
		);
	}

	// A failed background refetch also sets `isError`, so gate on having nothing
	// to show: never blank a rendered list because one 5s poll blipped.
	if (isError && bundles.length === 0) {
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

	if (bundles.length === 0) {
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
					className="flex-1 overflow-y-auto overflow-x-hidden"
				>
					{bundles.map((bundle) => {
						const stats = getBundleStats(bundle.responses);
						const firstCommand = bundle.responses[0];
						const { label: commandLabel } = firstCommand
							? getTxLabel(firstCommand.cmd_data)
							: { label: "Unknown Command" };
						const isSelected = bundle.uuid === selectedUuid;

						return (
							<button
								key={bundle.uuid}
								type="button"
								onClick={() => setSelectedUuid(bundle.uuid)}
								className={`w-full text-left px-4 py-3 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
									isSelected
										? "bg-blue-50 border-l-2 border-l-blue-500"
										: "hover:bg-gray-50 border-l-2 border-l-transparent"
								}`}
							>
								<div className="flex items-center gap-2 mb-1 min-w-0">
									<Terminal
										className={`w-3.5 h-3.5 shrink-0 ${isSelected ? "text-blue-500" : "text-purple-500"}`}
									/>
									<span
										className={`text-sm font-medium truncate min-w-0 ${isSelected ? "text-blue-900" : "text-gray-900"}`}
									>
										{commandLabel}
									</span>
								</div>
								<div className="flex items-center justify-between gap-2">
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
									<span className="text-xs text-gray-400 shrink-0 truncate max-w-[50%] text-right">
										{bundle.user_email ?? "System"}
									</span>
								</div>
								<div className="flex justify-end mt-0.5">
									<RelativeTime
										date={bundle.created_on}
										className="text-xs text-gray-400 shrink-0"
									/>
								</div>
							</button>
						);
					})}
				</div>
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
					<div className="flex items-center justify-center h-full text-gray-400 text-sm">
						Select a bundle to see its details
					</div>
				)}
			</div>
		</Card>,
	);
};

export default CommandsPage;
