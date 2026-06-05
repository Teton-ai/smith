import { Loader2, Send, Terminal } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router";
import {
	type BundleWithCommands,
	type BundleWithCommandsPaginated,
	type DeviceCommandResponse,
	useGetBundleCommandsInfinite,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { Button } from "@/app/components/button";
import { RelativeTime } from "@/app/components/RelativeTime";
import {
	CodeBlock,
	getCommandStatus,
	getStatusColor,
	getTxLabel,
	renderRxDetail,
	renderTxDetail,
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
	const status = getCommandStatus(response);
	const { label, mono } = getTxLabel(response.cmd_data);

	return (
		<div className="border-b border-gray-100 last:border-b-0">
			<div className="flex items-center justify-between gap-3 px-5 py-3 bg-gray-50">
				<div className="flex items-center gap-2 flex-wrap min-w-0">
					<span
						className={`text-sm font-medium text-gray-900 truncate ${mono ? "font-mono" : ""}`}
					>
						{label}
					</span>
					<span
						className={`px-2 py-0.5 text-xs font-medium rounded ${getStatusColor(status)}`}
					>
						{status}
					</span>
					{response.response != null && response.status != null && (
						<span
							className={`px-2 py-0.5 text-xs font-mono rounded ${response.status === 0 ? "bg-green-100 text-green-800" : "bg-red-100 text-red-800"}`}
						>
							exit {response.status}
						</span>
					)}
				</div>
				<div className="flex items-center gap-3 shrink-0">
					{response.response != null && (
						<Button
							variant="secondary"
							className="text-xs"
							onClick={() => setShowRaw((v) => !v)}
						>
							{showRaw ? "Formatted" : "Raw JSON"}
						</Button>
					)}
					<div className="text-xs text-gray-400">
						{response.response_at ? (
							<RelativeTime date={response.response_at} />
						) : (
							<span className="text-yellow-500">Waiting…</span>
						)}
					</div>
				</div>
			</div>
			<div className="px-5 py-3 space-y-3">
				{showRaw ? (
					<>
						<CodeBlock
							label="raw TX"
							content={JSON.stringify(response.cmd_data, null, 2)}
						/>
						<CodeBlock
							label="raw RX"
							content={JSON.stringify(response.response, null, 2)}
						/>
					</>
				) : (
					<>
						<div>
							<p className="text-xs font-medium uppercase tracking-wide text-gray-400 mb-2">
								Sent
							</p>
							{renderTxDetail(response.cmd_data)}
						</div>
						<div>
							<p className="text-xs font-medium uppercase tracking-wide text-gray-400 mb-2">
								Response
							</p>
							{renderRxDetail(response.response)}
						</div>
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
			<div className="w-2/5 border-r border-gray-200 overflow-y-auto shrink-0">
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
							<div className="flex items-center justify-between gap-2 mb-1">
								<span
									className={`text-sm font-mono truncate ${isSelected ? "text-blue-900" : "text-gray-900"}`}
								>
									{d.serial}
								</span>
								<span className="text-xs text-gray-400 shrink-0">
									{d.commands.length} {d.commands.length === 1 ? "cmd" : "cmds"}
								</span>
							</div>
							<div className="flex items-center gap-1.5 flex-wrap">
								{stats.success > 0 && (
									<span className="px-1.5 py-0.5 rounded-full bg-green-100 text-green-800 text-xs">
										{stats.success} ok
									</span>
								)}
								{stats.failed > 0 && (
									<span className="px-1.5 py-0.5 rounded-full bg-red-100 text-red-800 text-xs">
										{stats.failed} failed
									</span>
								)}
								{stats.pending > 0 && (
									<span className="px-1.5 py-0.5 rounded-full bg-yellow-100 text-yellow-800 text-xs">
										{stats.pending} pending
									</span>
								)}
								{stats.executing > 0 && (
									<span className="px-1.5 py-0.5 rounded-full bg-blue-100 text-blue-800 text-xs">
										{stats.executing} executing
									</span>
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
							<Link
								to={`/devices/${selected.serial}/commands`}
								className="text-sm font-mono font-medium text-blue-600 hover:underline"
							>
								{selected.serial}
							</Link>
						</div>
						<div className="flex-1 overflow-y-auto">
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

	const {
		data: bundleData,
		isLoading,
		fetchNextPage,
		hasNextPage,
		isFetchingNextPage,
	} = useGetBundleCommandsInfinite({
		query: {
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
					params: pageParam
						? { starting_after: pageParam, limit: PAGE_SIZE }
						: { limit: PAGE_SIZE },
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

	// Auto-select first bundle
	useEffect(() => {
		if (bundles.length > 0 && selectedUuid === null) {
			setSelectedUuid(bundles[0].uuid);
		}
	}, [bundles, selectedUuid]);

	const selectedBundle = bundles.find((b) => b.uuid === selectedUuid) ?? null;

	if (isLoading) {
		return (
			<div className="flex items-center justify-center py-12">
				<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
			</div>
		);
	}

	if (bundles.length === 0) {
		return (
			<div className="text-center py-12 bg-white border border-gray-200 rounded-lg">
				<Send className="w-12 h-12 text-gray-300 mx-auto mb-3" />
				<p className="text-gray-500">No bulk commands have been executed yet</p>
				<p className="text-sm text-gray-400 mt-1">
					Select devices and run a command to see results here
				</p>
			</div>
		);
	}

	return (
		<div className="flex-1 overflow-hidden p-4 sm:p-6 flex flex-col">
			<div className="flex-1 overflow-hidden flex border border-gray-200 bg-white rounded-lg">
				{/* Left: bundle list (1/3) */}
				<div className="w-1/5 border-r border-gray-200 shrink-0 flex flex-col overflow-hidden">
					<div
						ref={scrollRef}
						onScroll={handleScroll}
						className="flex-1 overflow-y-auto"
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
									<div className="flex items-center gap-2 mb-1">
										<Terminal
											className={`w-3.5 h-3.5 shrink-0 ${isSelected ? "text-blue-500" : "text-purple-500"}`}
										/>
										<span
											className={`text-sm font-medium truncate ${isSelected ? "text-blue-900" : "text-gray-900"}`}
										>
											{commandLabel}
										</span>
									</div>
									<div className="flex items-center justify-between gap-2">
										<div className="flex items-center gap-1.5 flex-wrap">
											{stats.success > 0 && (
												<span className="px-1.5 py-0.5 rounded-full bg-green-100 text-green-800 text-xs">
													{stats.success} ok
												</span>
											)}
											{stats.failed > 0 && (
												<span className="px-1.5 py-0.5 rounded-full bg-red-100 text-red-800 text-xs">
													{stats.failed} failed
												</span>
											)}
											{stats.pending > 0 && (
												<span className="px-1.5 py-0.5 rounded-full bg-yellow-100 text-yellow-800 text-xs">
													{stats.pending} pending
												</span>
											)}
											{stats.executing > 0 && (
												<span className="px-1.5 py-0.5 rounded-full bg-blue-100 text-blue-800 text-xs">
													{stats.executing} executing
												</span>
											)}
										</div>
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
			</div>
		</div>
	);
};

export default CommandsPage;
