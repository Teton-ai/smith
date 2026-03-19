"use client";

import { Loader2, Send, Terminal } from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import {
	type BundleWithCommands,
	type BundleWithCommandsPaginated,
	type DeviceCommandResponse,
	useGetBundleCommandsInfinite,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { Button } from "@/app/components/button";
import {
	CodeBlock,
	getCommandStatus,
	getStatusColor,
	getTxLabel,
	renderRxDetail,
	renderTxDetail,
} from "./shared";

const PAGE_SIZE = 100;

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
// Device response detail (right-right panel)
// ---------------------------------------------------------------------------

const DeviceResponseDetail = ({
	response,
}: {
	response: DeviceCommandResponse;
}) => {
	const [showRaw, setShowRaw] = useState(false);
	const status = getCommandStatus(response);

	// biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset on response change
	useEffect(() => {
		setShowRaw(false);
	}, [response.cmd_id]);

	return (
		<div className="h-full flex flex-col overflow-hidden">
			<div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 shrink-0">
				<div className="flex items-center gap-2 flex-wrap">
					<Link
						href={`/devices/${response.serial_number}/commands`}
						className="text-sm font-mono font-medium text-blue-600 hover:underline"
					>
						{response.serial_number}
					</Link>
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
				<div className="flex items-center gap-3">
					{response.response != null && (
						<Button
							variant="secondary"
							className="text-xs shrink-0"
							onClick={() => setShowRaw((v) => !v)}
						>
							{showRaw ? "Formatted" : "Raw JSON"}
						</Button>
					)}
					<div className="text-xs text-gray-400">
						{response.response_at ? (
							<span>Responded {moment(response.response_at).fromNow()}</span>
						) : (
							<span className="text-yellow-500">Waiting for response…</span>
						)}
					</div>
				</div>
			</div>
			<div className="flex-1 overflow-y-auto px-4 py-3">
				{showRaw ? (
					<>
						<CodeBlock
							label="raw TX"
							content={JSON.stringify(response.cmd_data, null, 2)}
						/>
						<div className="mt-4">
							<CodeBlock
								label="raw RX"
								content={JSON.stringify(response.response, null, 2)}
							/>
						</div>
					</>
				) : (
					<>
						<p className="text-xs font-medium uppercase tracking-wide text-gray-400 mb-3">
							Response
						</p>
						{renderRxDetail(response.response)}
					</>
				)}
			</div>
		</div>
	);
};

// ---------------------------------------------------------------------------
// Bundle detail (right panel): TX once + nested device split
// ---------------------------------------------------------------------------

const BundleDetail = ({ bundle }: { bundle: BundleWithCommands }) => {
	const [selectedDeviceId, setSelectedDeviceId] = useState<number>(
		bundle.responses[0]?.cmd_id ?? -1,
	);

	// If the selected device isn't in this bundle (e.g. bundle just changed),
	// sync state to the first response immediately to avoid a stale highlight.
	const firstResponseId = bundle.responses[0]?.cmd_id ?? -1;
	const responseInBundle = bundle.responses.find(
		(r) => r.cmd_id === selectedDeviceId,
	);
	if (!responseInBundle && selectedDeviceId !== firstResponseId) {
		setSelectedDeviceId(firstResponseId);
	}
	const selectedResponse = responseInBundle ?? bundle.responses[0] ?? null;
	const firstCommand = bundle.responses[0];

	return (
		<div className="h-full flex flex-col overflow-hidden">
			{/* TX section — same for all devices */}
			{firstCommand && (
				<div className="px-5 py-4 bg-gray-50 border-b border-gray-200 shrink-0">
					<p className="text-xs font-medium uppercase tracking-wide text-blue-400 mb-3">
						Sent
					</p>
					{renderTxDetail(firstCommand.cmd_data)}
				</div>
			)}

			{/* Nested split: device list | device response */}
			<div className="flex flex-1 overflow-hidden border-t border-gray-200">
				{/* Device list */}
				<div className="w-2/5 border-r border-gray-200 overflow-y-auto shrink-0">
					{bundle.responses.map((response) => {
						const status = getCommandStatus(response);
						const isSelected = response.cmd_id === selectedDeviceId;
						return (
							<button
								key={response.cmd_id}
								type="button"
								onClick={() => setSelectedDeviceId(response.cmd_id)}
								className={`w-full text-left px-4 py-3 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
									isSelected
										? "bg-blue-50 border-l-2 border-l-blue-500"
										: "hover:bg-gray-50 border-l-2 border-l-transparent"
								}`}
							>
								<div className="flex items-center justify-between gap-2">
									<span
										className={`text-sm font-mono truncate ${isSelected ? "text-blue-900" : "text-gray-900"}`}
									>
										{response.serial_number}
									</span>
									<span
										className={`px-2 py-0.5 text-xs font-medium rounded shrink-0 ${getStatusColor(status)}`}
									>
										{status}
									</span>
								</div>
								<div className="text-xs text-gray-400 mt-0.5">
									{response.response_at
										? moment(response.response_at).fromNow()
										: "Waiting…"}
								</div>
							</button>
						);
					})}
				</div>

				{/* Device response */}
				<div className="flex-1 overflow-hidden">
					{selectedResponse != null ? (
						<DeviceResponseDetail response={selectedResponse} />
					) : (
						<div className="flex items-center justify-center h-full text-gray-400 text-sm">
							Select a device to see its response
						</div>
					)}
				</div>
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

	const bundles = useMemo(
		() => (bundleData?.pages ?? []).flatMap((p) => p?.bundles ?? []),
		[bundleData],
	);

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
					<div className="flex-1 overflow-y-auto">
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
										<span className="text-xs text-gray-400 shrink-0">
											{moment(bundle.created_on).fromNow()}
										</span>
									</div>
								</button>
							);
						})}
					</div>
					{hasNextPage && (
						<div className="border-t border-gray-200 shrink-0">
							<Button
								variant="ghost"
								loading={isFetchingNextPage}
								onClick={() => fetchNextPage()}
								className="w-full py-2 px-4 text-sm font-medium text-blue-600 bg-blue-50 rounded-none hover:bg-blue-100"
							>
								{isFetchingNextPage ? "Loading more…" : "Load more"}
							</Button>
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
