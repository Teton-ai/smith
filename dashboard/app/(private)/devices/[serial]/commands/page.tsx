"use client";

import { ArrowLeft, Loader2, Send } from "lucide-react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	CodeBlock,
	getCommandStatus,
	getStatusColor,
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
import { Button } from "@/app/components/button";
import { RelativeTime } from "@/app/components/RelativeTime";
import DeviceHeader from "../DeviceHeader";

const PAGE_SIZE = 50;

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
			<div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 shrink-0">
				<div className="flex items-center gap-2 flex-wrap">
					<span className="text-sm font-medium text-gray-900">{txLabel}</span>
					<span
						className={`px-2 py-0.5 text-xs font-medium rounded ${getStatusColor(status)}`}
					>
						{status}
					</span>
					{cmd.response != null && cmd.status != null && (
						<span
							className={`px-2 py-0.5 text-xs font-mono rounded ${cmd.status === 0 ? "bg-green-100 text-green-800" : "bg-red-100 text-red-800"}`}
						>
							exit {cmd.status}
						</span>
					)}
				</div>
				<div className="flex items-center gap-3">
					{cmd.response != null && (
						<Button
							variant="secondary"
							className="text-xs shrink-0"
							onClick={() => setShowRaw((v) => !v)}
						>
							{showRaw ? "Formatted" : "Raw JSON"}
						</Button>
					)}
					<div className="flex items-center gap-2 text-xs text-gray-400">
						<span>
							Issued <RelativeTime date={cmd.issued_at} />
						</span>
						<span>·</span>
						{cmd.response_at ? (
							<span>
								Responded <RelativeTime date={cmd.response_at} />
							</span>
						) : (
							<span className="text-yellow-500">Waiting for response…</span>
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
							content={JSON.stringify(cmd.cmd_data, null, 2)}
						/>
						<div className="mt-4">
							<CodeBlock
								label="raw RX"
								content={JSON.stringify(cmd.response, null, 2)}
							/>
						</div>
					</>
				) : (
					<>
						<div className="mb-4">
							<p className="text-xs font-medium uppercase tracking-wide text-gray-400 mb-3">
								Sent
							</p>
							{renderTxDetail(cmd.cmd_data)}
						</div>
						<p className="text-xs font-medium uppercase tracking-wide text-gray-400 mb-3">
							Response
						</p>
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
	const router = useRouter();
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
		<div className="flex-1 overflow-hidden flex flex-col px-4 sm:px-6 lg:px-8 py-6">
			{/* Back link */}
			<div className="flex items-center space-x-4 mb-6">
				<button
					type="button"
					onClick={() => router.back()}
					className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
				>
					<ArrowLeft className="w-4 h-4" />
					<span className="text-sm font-medium">Back to Devices</span>
				</button>
			</div>

			{/* Device Header */}
			{device != null && (
				<div className="mb-6">
					<DeviceHeader device={device} serial={serial} />
				</div>
			)}

			{/* Tabs */}
			<div className="border-b border-gray-200 mb-6">
				<nav className="-mb-px flex space-x-8">
					<Link
						href={`/devices/${serial}`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Overview
					</Link>
					<button className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm">
						Commands
					</button>
					<Link
						href={`/devices/${serial}/services`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Services
					</Link>
				</nav>
			</div>

			{/* Main content */}
			<div className="flex-1 overflow-hidden">
				{loading ? (
					<div className="flex items-center justify-center py-12">
						<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
					</div>
				) : commands.length === 0 ? (
					<div className="text-center py-12 bg-white border border-gray-200 rounded-lg">
						<Send className="w-12 h-12 text-gray-300 mx-auto mb-3" />
						<p className="text-gray-500">No commands found</p>
						<p className="text-sm text-gray-400 mt-1">
							Run a command from the device header above
						</p>
					</div>
				) : (
					<div className="flex border border-gray-200 rounded-lg overflow-hidden bg-white h-full">
						{/* Left: command list */}
						<div className="w-1/3 border-r border-gray-200 shrink-0 flex flex-col overflow-hidden">
							<div
								ref={scrollRef}
								onScroll={handleScroll}
								className="flex-1 overflow-y-auto"
							>
								{commands.map((cmd) => {
									const status = getCommandStatus(cmd);
									const { label, mono } = getTxLabel(cmd.cmd_data);
									const isSelected = cmd.cmd_id === selectedId;

									return (
										<button
											key={cmd.cmd_id}
											type="button"
											onClick={() => setSelectedId(cmd.cmd_id)}
											className={`w-full text-left px-4 py-3 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
												isSelected
													? "bg-blue-50 border-l-2 border-l-blue-500"
													: "hover:bg-gray-50 border-l-2 border-l-transparent"
											}`}
										>
											<div className="flex items-center justify-between gap-2">
												<span
													className={`text-sm truncate ${mono ? "font-mono" : "font-medium"} ${isSelected ? "text-blue-900" : "text-gray-900"}`}
												>
													{label}
												</span>
												<span
													className={`px-2 py-0.5 text-xs font-medium rounded shrink-0 ${getStatusColor(status)}`}
												>
													{status}
												</span>
											</div>
											<div className="text-xs text-gray-400 mt-0.5">
												<RelativeTime date={cmd.issued_at} />
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
					</div>
				)}
			</div>
		</div>
	);
};

export default CommandsPage;
