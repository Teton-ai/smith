import {
	Badge,
	type BadgeVariant,
	Button,
	Card,
	ListRow,
} from "@teton/smith-ui";
import { Loader2, Send } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router";
import {
	CodeBlock,
	getCommandStatus,
	getTxLabel,
	renderRxDetail,
	renderTxDetail,
} from "@/app/(private)/commands/shared";
import {
	type CommandsPaginated,
	type DeviceCommandResponse,
	useGetAllCommandsForDeviceInfinite,
	useGetDeviceInfo,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { RelativeTime } from "@/app/components/RelativeTime";
import { DeviceDetailLayout } from "../DeviceDetailLayout";

const PAGE_SIZE = 50;

// Maps a command status to a design-system Badge variant.
const STATUS_VARIANT: Record<string, BadgeVariant> = {
	success: "green",
	failed: "red",
	executing: "blue",
	cancelled: "gray",
	pending: "yellow",
};
const statusVariant = (status: string): BadgeVariant =>
	STATUS_VARIANT[status] ?? "gray";

// ---------------------------------------------------------------------------
// Right panel: full detail view
// ---------------------------------------------------------------------------

const ResponseDetail = ({ cmd }: { cmd: DeviceCommandResponse }) => {
	const [showRaw, setShowRaw] = useState(false);
	const status = getCommandStatus(cmd);
	const { label: txLabel } = getTxLabel(cmd.cmd_data);

	// biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset on cmd change
	useEffect(() => {
		setShowRaw(false);
	}, [cmd.cmd_id]);

	return (
		<div className="h-full flex flex-col overflow-hidden">
			{/* Header */}
			<div className="px-4 py-3 border-b border-gray-200 bg-gray-50/70 shrink-0">
				<div className="flex items-center justify-between gap-3">
					<div className="flex items-center gap-2 flex-wrap min-w-0">
						<span className="text-sm font-semibold text-gray-900 truncate">
							{txLabel}
						</span>
						<Badge variant={statusVariant(status)}>{status}</Badge>
						{cmd.response != null && cmd.status != null && (
							<Badge
								variant={cmd.status === 0 ? "green" : "red"}
								className="font-mono"
							>
								exit {cmd.status}
							</Badge>
						)}
					</div>
					<div className="flex items-center gap-3 shrink-0">
						<p className="text-xs text-gray-500">
							Triggered by: {cmd.user_email ?? "System"}
						</p>
						{cmd.response != null && (
							<Button
								variant="soft"
								tone="gray"
								size="sm"
								onClick={() => setShowRaw((v) => !v)}
							>
								{showRaw ? "Formatted" : "Raw JSON"}
							</Button>
						)}
					</div>
				</div>
			</div>

			{/* Scrollable body */}
			<div className="flex-1 overflow-y-auto px-4 py-3">
				{showRaw ? (
					<>
						<CodeBlock
							label="raw TX"
							meta={
								<>
									· Issued <RelativeTime date={cmd.issued_at} />
								</>
							}
							content={JSON.stringify(cmd.cmd_data, null, 2)}
						/>
						<div className="mt-4">
							<CodeBlock
								label="raw RX"
								meta={
									cmd.response_at ? (
										<>
											· Responded <RelativeTime date={cmd.response_at} />
										</>
									) : (
										<span className="text-yellow-500">
											· Waiting for response…
										</span>
									)
								}
								content={JSON.stringify(cmd.response, null, 2)}
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
									· Issued <RelativeTime date={cmd.issued_at} />
								</span>
							</div>
							{renderTxDetail(cmd.cmd_data)}
						</div>
						<div className="flex items-center gap-2 mb-3">
							<p className="text-xs font-medium uppercase tracking-wide text-gray-400">
								Response
							</p>
							{cmd.response_at ? (
								<span className="text-xs text-gray-400">
									· Responded <RelativeTime date={cmd.response_at} />
								</span>
							) : (
								<span className="text-xs text-yellow-500">
									· Waiting for response…
								</span>
							)}
						</div>
						{renderRxDetail(cmd.response)}
					</>
				)}
			</div>
		</div>
	);
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

const CommandsPage = () => {
	const { serial } = useParams<{ serial: string }>();
	const [selectedId, setSelectedId] = useState<number | null>(null);
	const scrollRef = useRef<HTMLDivElement>(null);
	const fetcher = useClientMutator<CommandsPaginated>();

	const {
		data: commandsData,
		isLoading: commandsLoading,
		fetchNextPage,
		hasNextPage,
		isFetchingNextPage,
	} = useGetAllCommandsForDeviceInfinite(serial, undefined, {
		query: {
			initialPageParam: undefined as number | undefined,
			getNextPageParam: (lastPage) => {
				if (!lastPage?.next) return undefined;
				const url = new URL(lastPage.next);
				const val = url.searchParams.get("starting_after");
				return val ? Number(val) : undefined;
			},
			queryFn: ({ signal, pageParam }) =>
				fetcher({
					url: `/devices/${serial}/commands`,
					method: "GET",
					params: pageParam
						? { starting_after: pageParam, limit: PAGE_SIZE }
						: { limit: PAGE_SIZE },
					signal,
				}),
			refetchInterval: 5000,
		},
	});

	const { data: device, isLoading: deviceLoading } = useGetDeviceInfo(serial);

	const handleScroll = useCallback(() => {
		const el = scrollRef.current;
		if (!el || !hasNextPage || isFetchingNextPage) return;
		if (el.scrollHeight - el.scrollTop - el.clientHeight < 600) {
			fetchNextPage();
		}
	}, [hasNextPage, isFetchingNextPage, fetchNextPage]);

	const commands = useMemo(() => {
		const all = (commandsData?.pages ?? []).flatMap((p) => p?.commands ?? []);
		const seen = new Set<number>();
		return all.filter((c) => {
			if (seen.has(c.cmd_id)) return false;
			seen.add(c.cmd_id);
			return true;
		});
	}, [commandsData]);
	const loading = commandsLoading || deviceLoading;

	useEffect(() => {
		if (commands.length > 0 && selectedId === null) {
			setSelectedId(commands[0].cmd_id);
		}
	}, [commands, selectedId]);

	const selectedCmd = commands.find((c) => c.cmd_id === selectedId) ?? null;

	return (
		<DeviceDetailLayout
			serial={serial ?? ""}
			device={device ?? undefined}
			activeTab="commands"
			fill
		>
			{/* Main content */}
			{loading ? (
				<div className="flex items-center justify-center py-12">
					<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
				</div>
			) : commands.length === 0 ? (
				<Card className="text-center py-12">
					<Send className="w-12 h-12 text-gray-300 mx-auto mb-3" />
					<p className="text-gray-500">No commands found</p>
					<p className="text-sm text-gray-400 mt-1">
						Run a command from the device header above
					</p>
				</Card>
			) : (
				<Card className="flex overflow-hidden h-full">
					{/* Left: command list */}
					<div className="w-1/3 border-r border-gray-200 shrink-0 flex flex-col overflow-hidden">
						<div
							ref={scrollRef}
							onScroll={handleScroll}
							className="flex-1 overflow-y-auto divide-y divide-gray-100"
						>
							{commands.map((cmd) => {
								const status = getCommandStatus(cmd);
								const { label, mono } = getTxLabel(cmd.cmd_data);
								const isSelected = cmd.cmd_id === selectedId;

								return (
									<ListRow
										key={cmd.cmd_id}
										onClick={() => setSelectedId(cmd.cmd_id)}
										hover={isSelected ? "bg-blue-50" : "hover:bg-gray-50"}
										className={
											isSelected
												? "border-l-2 border-l-blue-500"
												: "border-l-2 border-l-transparent"
										}
									>
										<div className="w-full min-w-0">
											<div className="flex items-center justify-between gap-2">
												<span
													className={`text-sm truncate ${mono ? "font-mono" : "font-medium"} ${isSelected ? "text-blue-900" : "text-gray-900"}`}
												>
													{label}
												</span>
												<Badge
													variant={statusVariant(status)}
													className="shrink-0"
												>
													{status}
												</Badge>
											</div>
											<div className="text-xs text-gray-400 mt-0.5">
												<RelativeTime date={cmd.issued_at} />
											</div>
										</div>
									</ListRow>
								);
							})}
						</div>
						{isFetchingNextPage && (
							<div className="flex items-center justify-center py-3 border-t border-gray-200 shrink-0">
								<Loader2 className="w-4 h-4 animate-spin text-gray-400" />
							</div>
						)}
					</div>

					{/* Right: detail */}
					<div className="flex-1 overflow-hidden">
						{selectedCmd != null ? (
							<ResponseDetail cmd={selectedCmd} />
						) : (
							<div className="flex items-center justify-center h-full text-gray-400 text-sm">
								Select a command to see its output
							</div>
						)}
					</div>
				</Card>
			)}
		</DeviceDetailLayout>
	);
};

export default CommandsPage;
