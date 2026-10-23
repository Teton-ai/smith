import { useQuery } from "@tanstack/react-query";
import { Badge, Button, Card } from "@teton/smith-ui";
import { Loader2, Send } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router";
import {
	SelectionMessage,
	selectionSearch,
	useRevealSelection,
	validCommandId,
} from "@/app/(private)/commands/selection";
import {
	CommandBody,
	CommandHeader,
	CommandListRow,
	CommandMeta,
	getCommandStatus,
	getTxLabel,
	statusVariant,
} from "@/app/(private)/commands/shared";
import {
	type CommandsPaginated,
	type DeviceCommandResponse,
	useGetAllCommandsForDeviceInfinite,
	useGetDeviceInfo,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { DeviceDetailLayout } from "../DeviceDetailLayout";

const PAGE_SIZE = 50;

// ---------------------------------------------------------------------------
// Right panel: full detail view
// ---------------------------------------------------------------------------

const ResponseDetail = ({ cmd }: { cmd: DeviceCommandResponse }) => {
	const [showRaw, setShowRaw] = useState(false);

	// biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset on cmd change
	useEffect(() => {
		setShowRaw(false);
	}, [cmd.cmd_id]);

	return (
		<div className="h-full flex flex-col overflow-hidden">
			<CommandHeader
				cmd={cmd}
				showRaw={showRaw}
				onToggleRaw={() => setShowRaw((v) => !v)}
				className="border-b border-gray-200 shrink-0"
			>
				<CommandMeta trigger={cmd.user_email} bundleId={cmd.bundle_id} />
			</CommandHeader>
			<div className="flex-1 overflow-y-auto px-5 py-3">
				<CommandBody cmd={cmd} showRaw={showRaw} />
			</div>
		</div>
	);
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

const CommandsPage = () => {
	const { serial = "" } = useParams<{ serial: string }>();
	const [searchParams, setSearchParams] = useSearchParams();
	const id = searchParams.get("id");
	const validId = validCommandId(id);
	const selectedId = validId ? Number(id) : null;
	const detailFetcher = useClientMutator<DeviceCommandResponse>();
	const scrollRef = useRef<HTMLDivElement>(null);
	const fetcher = useClientMutator<CommandsPaginated>();

	const {
		data: commandsData,
		isLoading: commandsLoading,
		isError: commandsError,
		fetchNextPage,
		hasNextPage,
		isFetchingNextPage,
		isFetching,
		isFetchNextPageError,
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
		if (commands.length > 0 && id === null) {
			setSearchParams(
				selectionSearch(searchParams, { id: String(commands[0].cmd_id) }),
				{ replace: true },
			);
		}
	}, [commands, id, searchParams, setSearchParams]);

	const detail = useQuery({
		queryKey: [`/devices/${serial}/commands/${selectedId}`],
		queryFn: ({ signal }) =>
			detailFetcher({
				url: `/devices/${serial}/commands/${selectedId}`,
				method: "GET",
				signal,
			}),
		enabled: validId && !!serial,
		refetchInterval: 5000,
	});
	const selectedCmd =
		detail.data ?? commands.find((c) => c.cmd_id === selectedId) ?? null;
	useRevealSelection({
		selectionKey: selectedId === null ? null : `${serial}:${selectedId}`,
		containerRef: scrollRef,
		enabled: selectedCmd !== null,
		hasNextPage,
		isFetching,
		isFetchNextPageError,
		fetchNextPage,
	});

	return (
		<DeviceDetailLayout
			serial={serial ?? ""}
			device={device ?? undefined}
			activeTab="commands"
			fill
		>
			{/* Main content */}
			{loading && id === null ? (
				<div className="flex items-center justify-center py-12">
					<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
				</div>
			) : commands.length === 0 && id === null ? (
				<Card className="text-center py-12">
					<Send className="w-12 h-12 text-gray-300 mx-auto mb-3" />
					<p className="text-gray-500">
						{commandsError ? "Failed to load commands" : "No commands found"}
					</p>
					<p className="text-sm text-gray-400 mt-1">
						{commandsError
							? "The request will retry automatically."
							: "Run a command from the device header above"}
					</p>
				</Card>
			) : (
				<Card className="flex overflow-hidden h-full">
					{/* Left: command list */}
					<div className="w-1/3 border-r border-gray-200 shrink-0 flex flex-col overflow-hidden">
						<div
							ref={scrollRef}
							onScroll={handleScroll}
							className="flex-1 overflow-y-auto overflow-x-hidden divide-y divide-gray-100"
						>
							{commands.map((cmd) => {
								const status = getCommandStatus(cmd);
								const { label, mono } = getTxLabel(cmd.cmd_data);

								return (
									<CommandListRow
										key={cmd.cmd_id}
										selected={cmd.cmd_id === selectedId}
										to={selectionSearch(searchParams, {
											id: String(cmd.cmd_id),
										})}
										title={label}
										mono={mono}
										badge={
											<Badge variant={statusVariant(status)}>{status}</Badge>
										}
										trigger={cmd.user_email}
										time={cmd.issued_at}
									/>
								);
							})}
						</div>
						{isFetchNextPageError && (
							<Button
								variant="soft"
								tone="gray"
								onClick={() => fetchNextPage()}
							>
								Retry loading history
							</Button>
						)}
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
							<SelectionMessage
								invalid={id !== null && !validId}
								loading={validId && detail.isPending}
								error={detail.error}
								kind="command"
							/>
						)}
					</div>
				</Card>
			)}
		</DeviceDetailLayout>
	);
};

export default CommandsPage;
