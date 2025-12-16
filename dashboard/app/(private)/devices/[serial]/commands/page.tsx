"use client";

import { ArrowLeft, Check, Copy, Reply, Send } from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useState } from "react";
import { useGetAllCommandsForDevice, useGetDeviceInfo } from "@/app/api-client";
import DeviceHeader from "../DeviceHeader";

interface Command {
	[key: string]: any;
}

const CommandsPage = () => {
	const params = useParams();
	const [copiedButtons, setCopiedButtons] = useState<Set<string>>(new Set());

	const serial = params.serial as string;

	const { data: commandsData, isLoading: commandsLoading } =
		useGetAllCommandsForDevice(serial, { limit: 500 });

	const { data: device, isLoading: deviceLoading } = useGetDeviceInfo(serial);

	const commands = commandsData?.commands || [];
	const loading = commandsLoading || deviceLoading;

	const getCommandDisplay = (cmd: Command) => {
		if (typeof cmd.cmd_data === "string") {
			return { type: cmd.cmd_data, content: null };
		}
		if (typeof cmd.cmd_data === "object" && cmd.cmd_data) {
			const type = Object.keys(cmd.cmd_data)[0];
			const content = cmd.cmd_data[type];
			return { type, content };
		}
		return { type: "Unknown", content: null };
	};

	const getResponseDisplay = (cmd: Command) => {
		if (cmd.response === null) return { type: "None", content: null };
		if (typeof cmd.response === "string") {
			return { type: cmd.response, content: null };
		}
		if (typeof cmd.response === "object" && cmd.response) {
			const type = Object.keys(cmd.response)[0];
			const content = cmd.response[type];
			return { type, content };
		}
		return { type: "Unknown", content: null };
	};

	const getCommandStatus = (cmd: Command) => {
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

	const copyToClipboard = (content: any, buttonId: string) => {
		navigator.clipboard.writeText(JSON.stringify(content, null, 2));

		// Show copied state
		setCopiedButtons((prev) => new Set([...prev, buttonId]));

		// Reset after 2 seconds
		setTimeout(() => {
			setCopiedButtons((prev) => {
				const newSet = new Set(prev);
				newSet.delete(buttonId);
				return newSet;
			});
		}, 2000);
	};

	return (
		<div className="space-y-6">
			{/* Header with Back Button */}
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
				</nav>
			</div>

			{/* Commands Content */}
			{loading ? (
				<div className="p-6 text-gray-500">Loading...</div>
			) : commands.length === 0 ? (
				<div className="p-6 text-gray-500">No commands found</div>
			) : (
				<div className="space-y-3">
					{commands.map((cmd) => {
						const status = getCommandStatus(cmd);
						const commandDisplay = getCommandDisplay(cmd);
						const responseDisplay = getResponseDisplay(cmd);

						return (
							<div
								key={cmd.cmd_id}
								className="border border-gray-200 rounded-lg p-4 bg-white"
							>
								{/* Header with command info */}
								<div className="flex items-center space-x-3 mb-2">
									<Send className="w-4 h-4 text-gray-500" />
									<span className="font-medium text-gray-900">
										{commandDisplay.type}
									</span>
									<span
										className={`px-2 py-1 text-xs font-medium rounded ${getStatusColor(status)}`}
									>
										{status}
									</span>
								</div>

								{/* Command and Response in side-by-side layout */}
								<div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
									{/* Command Content */}
									{commandDisplay.content && (
										<div>
											<div className="flex items-center space-x-2 mb-1">
												<div className="text-xs font-medium text-gray-600">
													Command
												</div>
												<span className="text-xs text-gray-500">
													{moment(cmd.issued_at).fromNow()}
												</span>
											</div>
											<div className="relative group">
												<pre className="text-xs font-mono bg-gray-900 text-gray-100 p-2 rounded overflow-x-auto whitespace-pre-wrap">
													{JSON.stringify(commandDisplay.content, null, 2)}
												</pre>
												<button
													id={`copy-cmd-${cmd.cmd_id}`}
													onClick={() =>
														copyToClipboard(
															commandDisplay.content,
															`copy-cmd-${cmd.cmd_id}`,
														)
													}
													className="absolute top-2 right-2 text-gray-400 hover:text-white hover:bg-gray-700 p-1 rounded transition-all opacity-0 group-hover:opacity-100 cursor-pointer"
													title={
														copiedButtons.has(`copy-cmd-${cmd.cmd_id}`)
															? "Copied!"
															: "Copy to clipboard"
													}
												>
													{copiedButtons.has(`copy-cmd-${cmd.cmd_id}`) ? (
														<Check className="w-3 h-3 text-green-400" />
													) : (
														<Copy className="w-3 h-3" />
													)}
												</button>
											</div>
										</div>
									)}

									{/* Response Content */}
									<div>
										<div className="flex items-center space-x-2 mb-1">
											<Reply className="w-3 h-3 text-gray-500" />
											<div className="text-xs font-medium text-gray-600">
												{responseDisplay.type}
											</div>
											{cmd.response_at && (
												<span className="text-xs text-gray-500">
													{moment(cmd.response_at).fromNow()}
												</span>
											)}
										</div>
										{responseDisplay.content && (
											<div className="relative group">
												<pre className="text-xs font-mono bg-gray-900 text-gray-100 p-2 rounded overflow-x-auto whitespace-pre-wrap">
													{JSON.stringify(responseDisplay.content, null, 2)}
												</pre>
												<button
													id={`copy-resp-${cmd.cmd_id}`}
													onClick={() =>
														copyToClipboard(
															responseDisplay.content,
															`copy-resp-${cmd.cmd_id}`,
														)
													}
													className="absolute top-2 right-2 text-gray-400 hover:text-white hover:bg-gray-700 p-1 rounded transition-all opacity-0 group-hover:opacity-100 cursor-pointer"
													title={
														copiedButtons.has(`copy-resp-${cmd.cmd_id}`)
															? "Copied!"
															: "Copy to clipboard"
													}
												>
													{copiedButtons.has(`copy-resp-${cmd.cmd_id}`) ? (
														<Check className="w-3 h-3 text-green-400" />
													) : (
														<Copy className="w-3 h-3" />
													)}
												</button>
											</div>
										)}
									</div>
								</div>
							</div>
						);
					})}
				</div>
			)}
		</div>
	);
};

export default CommandsPage;
