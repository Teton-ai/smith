"use client";

import {
	Check,
	ChevronDown,
	ChevronRight,
	Clock,
	Copy,
	Loader2,
	Send,
	Terminal,
} from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useState } from "react";
import {
	type BundleWithCommands,
	type DeviceCommandResponse,
	useGetBundleCommands,
} from "@/app/api-client";

const getCommandDisplay = (cmdData: unknown) => {
	if (typeof cmdData === "string") {
		return { type: cmdData, content: null };
	}
	if (typeof cmdData === "object" && cmdData) {
		const type = Object.keys(cmdData)[0];
		const content = (cmdData as Record<string, unknown>)[type];
		return { type, content };
	}
	return { type: "Unknown", content: null };
};

const getCommandStatus = (cmd: DeviceCommandResponse) => {
	if (cmd.cancelled) return "cancelled";
	if (!cmd.fetched) return "pending";
	if (!cmd.response_at) return "executing";
	return cmd.status === 0 ? "success" : "failed";
};

const getStatusColor = (status: string) => {
	switch (status) {
		case "success":
			return "bg-green-100 text-green-800";
		case "failed":
			return "bg-red-100 text-red-800";
		case "executing":
			return "bg-blue-100 text-blue-800";
		case "cancelled":
			return "bg-gray-100 text-gray-800";
		case "pending":
			return "bg-yellow-100 text-yellow-800";
		default:
			return "bg-gray-100 text-gray-800";
	}
};

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

const BundleCard = ({ bundle }: { bundle: BundleWithCommands }) => {
	const [expanded, setExpanded] = useState(false);
	const [copiedId, setCopiedId] = useState<string | null>(null);

	const stats = getBundleStats(bundle.responses);
	const firstCommand = bundle.responses[0];
	const commandDisplay = firstCommand
		? getCommandDisplay(firstCommand.cmd_data)
		: null;

	const previewDevices = bundle.responses.slice(0, 5);
	const remainingCount = bundle.responses.length - 5;

	const copyToClipboard = (content: unknown, id: string) => {
		navigator.clipboard.writeText(
			typeof content === "string" ? content : JSON.stringify(content, null, 2),
		);
		setCopiedId(id);
		setTimeout(() => setCopiedId(null), 2000);
	};

	return (
		<div className="border border-gray-200 rounded-lg bg-white overflow-hidden">
			<button
				onClick={() => setExpanded(!expanded)}
				className="w-full px-4 py-3 flex items-center justify-between hover:bg-gray-50 cursor-pointer"
			>
				<div className="flex items-center gap-4">
					{expanded ? (
						<ChevronDown className="w-4 h-4 text-gray-500" />
					) : (
						<ChevronRight className="w-4 h-4 text-gray-500" />
					)}
					<div className="flex items-center gap-2">
						<Terminal className="w-4 h-4 text-purple-600" />
						<span className="font-medium text-gray-900">
							{commandDisplay?.type || "Unknown Command"}
						</span>
					</div>
					<span className="text-sm text-gray-500">
						{moment(bundle.created_on).fromNow()}
					</span>
				</div>

				<div className="flex items-center gap-3">
					<div className="flex items-center gap-2 text-sm">
						{stats.success > 0 && (
							<span className="px-2 py-0.5 rounded-full bg-green-100 text-green-800 text-xs">
								{stats.success} success
							</span>
						)}
						{stats.failed > 0 && (
							<span className="px-2 py-0.5 rounded-full bg-red-100 text-red-800 text-xs">
								{stats.failed} failed
							</span>
						)}
						{stats.pending > 0 && (
							<span className="px-2 py-0.5 rounded-full bg-yellow-100 text-yellow-800 text-xs">
								{stats.pending} pending
							</span>
						)}
						{stats.executing > 0 && (
							<span className="px-2 py-0.5 rounded-full bg-blue-100 text-blue-800 text-xs">
								{stats.executing} executing
							</span>
						)}
					</div>
				</div>
			</button>

			{/* Device preview (always visible) */}
			{!expanded && (
				<div className="px-4 pb-3 flex flex-wrap gap-2">
					{previewDevices.map((response) => {
						const status = getCommandStatus(response);
						return (
							<Link
								key={response.cmd_id}
								href={`/devices/${response.serial_number}/commands`}
								onClick={(e) => e.stopPropagation()}
								className={`px-2 py-1 text-xs font-mono rounded border ${
									status === "success"
										? "bg-green-50 border-green-200 text-green-700"
										: status === "failed"
											? "bg-red-50 border-red-200 text-red-700"
											: status === "pending"
												? "bg-yellow-50 border-yellow-200 text-yellow-700"
												: status === "executing"
													? "bg-blue-50 border-blue-200 text-blue-700"
													: "bg-gray-50 border-gray-200 text-gray-700"
								} hover:opacity-80`}
							>
								{response.serial_number}
							</Link>
						);
					})}
					{remainingCount > 0 && (
						<span className="px-2 py-1 text-xs text-gray-500">
							+{remainingCount} more
						</span>
					)}
				</div>
			)}

			{expanded && (
				<div className="border-t border-gray-200">
					{/* Command Details */}
					{commandDisplay?.content && (
						<div className="px-4 py-3 bg-gray-50 border-b border-gray-200">
							<div className="flex items-center justify-between mb-1">
								<span className="text-xs font-medium text-gray-600">
									Command
								</span>
								<button
									onClick={() =>
										copyToClipboard(
											commandDisplay.content,
											`cmd-${bundle.uuid}`,
										)
									}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
								>
									{copiedId === `cmd-${bundle.uuid}` ? (
										<Check className="w-3 h-3 text-green-600" />
									) : (
										<Copy className="w-3 h-3" />
									)}
								</button>
							</div>
							<pre className="text-sm font-mono bg-gray-900 text-gray-100 p-2 rounded overflow-x-auto">
								{typeof commandDisplay.content === "object" &&
								commandDisplay.content !== null &&
								"cmd" in commandDisplay.content
									? String((commandDisplay.content as { cmd: string }).cmd)
									: JSON.stringify(commandDisplay.content, null, 2)}
							</pre>
						</div>
					)}

					{/* Device Responses */}
					<div className="divide-y divide-gray-100">
						{bundle.responses.map((response) => {
							const status = getCommandStatus(response);
							const responseDisplay =
								response.response != null
									? typeof response.response === "object"
										? response.response
										: response.response
									: null;

							return (
								<div key={response.cmd_id} className="px-4 py-3">
									<div className="flex items-center justify-between mb-2">
										<div className="flex items-center gap-2">
											<Link
												href={`/devices/${response.serial_number}/commands`}
												className="text-sm font-medium text-blue-600 hover:underline"
											>
												{response.serial_number}
											</Link>
											<span
												className={`px-2 py-0.5 text-xs font-medium rounded ${getStatusColor(status)}`}
											>
												{status}
											</span>
										</div>
										<div className="flex items-center gap-3 text-xs text-gray-500">
											{response.fetched_at && (
												<span className="flex items-center gap-1">
													<Clock className="w-3 h-3" />
													Fetched {moment(response.fetched_at).fromNow()}
												</span>
											)}
											{response.response_at && (
												<span className="flex items-center gap-1">
													<Check className="w-3 h-3" />
													Responded {moment(response.response_at).fromNow()}
												</span>
											)}
										</div>
									</div>

									{responseDisplay && (
										<div className="relative group">
											<pre className="text-xs font-mono bg-gray-900 text-gray-100 p-2 rounded overflow-x-auto whitespace-pre-wrap max-h-40">
												{typeof responseDisplay === "string"
													? responseDisplay
													: JSON.stringify(responseDisplay, null, 2)}
											</pre>
											<button
												onClick={() =>
													copyToClipboard(
														responseDisplay,
														`resp-${response.cmd_id}`,
													)
												}
												className="absolute top-2 right-2 text-gray-400 hover:text-white p-1 rounded opacity-0 group-hover:opacity-100 cursor-pointer"
											>
												{copiedId === `resp-${response.cmd_id}` ? (
													<Check className="w-3 h-3 text-green-400" />
												) : (
													<Copy className="w-3 h-3" />
												)}
											</button>
										</div>
									)}
								</div>
							);
						})}
					</div>
				</div>
			)}
		</div>
	);
};

const CommandsPage = () => {
	const { data: bundleData, isLoading } = useGetBundleCommands({
		query: { refetchInterval: 5000 },
	});

	const bundles = bundleData?.bundles || [];

	return (
		<div className="space-y-6">
			{isLoading ? (
				<div className="flex items-center justify-center py-12">
					<Loader2 className="w-6 h-6 animate-spin text-gray-400" />
				</div>
			) : bundles.length === 0 ? (
				<div className="text-center py-12 bg-white border border-gray-200 rounded-lg">
					<Send className="w-12 h-12 text-gray-300 mx-auto mb-3" />
					<p className="text-gray-500">
						No bulk commands have been executed yet
					</p>
					<p className="text-sm text-gray-400 mt-1">
						Select devices and run a command to see results here
					</p>
				</div>
			) : (
				<div className="space-y-3">
					{bundles.map((bundle) => (
						<BundleCard key={bundle.uuid} bundle={bundle} />
					))}
				</div>
			)}
		</div>
	);
};

export default CommandsPage;
