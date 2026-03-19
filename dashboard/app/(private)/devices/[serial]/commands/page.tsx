"use client";


import { ArrowLeft, Send } from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import {
	CodeBlock,
	getCommandStatus,
	getStatusColor,
	getTxLabel,
	renderRxDetail,
	renderTxDetail,
} from "@/app/(private)/commands/shared";
import {
	type DeviceCommandResponse,
	useGetAllCommandsForDevice,
	useGetDeviceInfo,
} from "@/app/api-client";
import { Button } from "@/app/components/button";
import DeviceHeader from "../DeviceHeader";

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
			<div className="flex items-start justify-between px-5 py-4 border-b border-gray-200 shrink-0">
				<div className="space-y-1">
					<div className="flex items-center gap-2 flex-wrap">
						<span className="font-semibold text-gray-900">{txLabel}</span>
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
					<div className="flex items-center gap-2 text-xs text-gray-400 flex-wrap">
						<span>Issued {moment(cmd.issued_at).fromNow()}</span>
						<span>·</span>
						{cmd.response_at ? (
							<span>Responded {moment(cmd.response_at).fromNow()}</span>
						) : (
							<span className="text-yellow-500">Waiting for response…</span>
						)}
					</div>
				</div>

				{cmd.response != null && (
					<Button
						variant="secondary"
						className="text-xs shrink-0 ml-4"
						onClick={() => setShowRaw((v) => !v)}
					>
						{showRaw ? "Formatted" : "Raw JSON"}
					</Button>
				)}
			</div>

			{/* Scrollable body */}
			<div className="flex-1 overflow-y-auto divide-y divide-gray-100">
				{showRaw ? (
					<div className="px-5 py-4">
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
					</div>
				) : (
					<>
						{/* Command sent */}
						<div className="px-5 py-4">
							<p className="text-xs font-medium uppercase tracking-wide text-blue-400 mb-3">
								Sent
							</p>
							{renderTxDetail(cmd.cmd_data)}
						</div>

						{/* Response */}
						<div className="px-5 py-4">
							<p className="text-xs font-medium uppercase tracking-wide text-gray-400 mb-3">
								Response
							</p>
							{renderRxDetail(cmd.response)}
						</div>
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

	const { data: commandsData, isLoading: commandsLoading } =
		useGetAllCommandsForDevice(serial, { limit: 500 });

	const { data: device, isLoading: deviceLoading } = useGetDeviceInfo(serial);

	const commands = commandsData?.commands ?? [];
	const loading = commandsLoading || deviceLoading;

	useEffect(() => {
		if (commands.length > 0 && selectedId === null) {
			setSelectedId(commands[0].cmd_id);
		}
	}, [commands, selectedId]);

	const selectedCmd = commands.find((c) => c.cmd_id === selectedId) ?? null;

	return (
		<div className="flex-1 overflow-y-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
			{/* Back link */}
			<div className="flex items-center space-x-4">
				<Link
					href="/devices"
					className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
				>
					<ArrowLeft className="w-4 h-4" />
					<span className="text-sm font-medium">Back to Devices</span>
				</Link>
			</div>

			{/* Device Header */}
			{device != null && <DeviceHeader device={device} serial={serial} />}

			{/* Tabs */}
			<div className="border-b border-gray-200">
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
			<div>
				{loading ? (
					<div className="p-6 text-gray-500 text-sm">Loading…</div>
				) : commands.length === 0 ? (
					<div className="flex flex-col items-center justify-center py-16 text-center">
						<Send className="w-10 h-10 text-gray-300 mb-3" />
						<p className="text-gray-500">No commands found</p>
						<p className="text-sm text-gray-400 mt-1">
							Run a command from the device header above
						</p>
					</div>
				) : (
				<div className="flex border border-gray-200 rounded-lg overflow-hidden bg-white min-h-[500px]">
						{/* Left: command list (1/3) */}
						<div className="w-1/3 border-r border-gray-200 overflow-y-auto shrink-0">
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
											{moment(cmd.issued_at).fromNow()}
										</div>
									</button>
								);
							})}
						</div>

						{/* Right: detail (2/3) */}
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
