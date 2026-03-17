"use client";

import { ArrowLeft, Check, Copy, Send } from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import {
	type DeviceCommandResponse,
	useGetAllCommandsForDevice,
	useGetDeviceInfo,
} from "@/app/api-client";
import { Button } from "@/app/components/button";
import DeviceHeader from "../DeviceHeader";

// ---------------------------------------------------------------------------
// SafeCommandTx types (mirrors smithd/src/utils/schema.rs)
// ---------------------------------------------------------------------------

type CmdTxPing = "Ping";
type CmdTxUpgrade = "Upgrade";
type CmdTxRestart = "Restart";
type CmdTxCloseTunnel = "CloseTunnel";
type CmdTxCheckOTAStatus = "CheckOTAStatus";
type CmdTxStartOTA = "StartOTA";
type CmdTxTestNetwork = "TestNetwork";

type CmdTxFreeForm = { FreeForm: { cmd: string } };
type CmdTxOpenTunnel = {
	OpenTunnel: {
		port?: number | null;
		user?: string | null;
		pub_key?: string | null;
	};
};
type CmdTxUpdateNetwork = {
	UpdateNetwork: {
		network: { name?: string; network_type?: string; [k: string]: unknown };
	};
};
type CmdTxUpdateVariables = {
	UpdateVariables: { variables: Record<string, string> };
};
type CmdTxDownloadOTA = {
	DownloadOTA: { tools: string; payload: string; rate: number };
};
type CmdTxExtendedNetworkTest = {
	ExtendedNetworkTest: { duration_minutes: number };
};
type CmdTxStreamLogs = {
	StreamLogs: { session_id: string; service_name: string };
};
type CmdTxStopLogStream = { StopLogStream: { session_id: string } };

type SafeCommandTx =
	| CmdTxPing
	| CmdTxUpgrade
	| CmdTxRestart
	| CmdTxCloseTunnel
	| CmdTxCheckOTAStatus
	| CmdTxStartOTA
	| CmdTxTestNetwork
	| CmdTxFreeForm
	| CmdTxOpenTunnel
	| CmdTxUpdateNetwork
	| CmdTxUpdateVariables
	| CmdTxDownloadOTA
	| CmdTxExtendedNetworkTest
	| CmdTxStreamLogs
	| CmdTxStopLogStream;

// Parse cmd_data into a typed discriminated union
const parseTx = (
	cmd_data: unknown,
): { variant: string; tx: SafeCommandTx } | null => {
	if (cmd_data == null) return null;
	if (typeof cmd_data === "string")
		return { variant: cmd_data, tx: cmd_data as SafeCommandTx };
	if (typeof cmd_data === "object") {
		const variant = Object.keys(cmd_data as object)[0];
		return { variant, tx: cmd_data as SafeCommandTx };
	}
	return null;
};

// Short label for the left list
const getTxLabel = (cmd_data: unknown): { label: string; mono: boolean } => {
	const parsed = parseTx(cmd_data);
	if (parsed == null) return { label: "Unknown", mono: false };

	switch (parsed.variant) {
		case "Ping":
			return { label: "Ping", mono: false };
		case "Upgrade":
			return { label: "Upgrade", mono: false };
		case "Restart":
			return { label: "Restart", mono: false };
		case "CloseTunnel":
			return { label: "Close Tunnel", mono: false };
		case "CheckOTAStatus":
			return { label: "Check OTA Status", mono: false };
		case "StartOTA":
			return { label: "Start OTA", mono: false };
		case "TestNetwork":
			return { label: "Test Network", mono: false };
		case "FreeForm": {
			const p = (parsed.tx as CmdTxFreeForm).FreeForm;
			return { label: p.cmd, mono: true };
		}
		case "OpenTunnel": {
			const p = (parsed.tx as CmdTxOpenTunnel).OpenTunnel;
			const suffix = p.port != null ? ` :${p.port}` : "";
			return { label: `Open Tunnel${suffix}`, mono: false };
		}
		case "UpdateNetwork": {
			const p = (parsed.tx as CmdTxUpdateNetwork).UpdateNetwork;
			const name = p.network?.name ?? "";
			return { label: `Update Network${name ? `: ${name}` : ""}`, mono: false };
		}
		case "UpdateVariables": {
			const p = (parsed.tx as CmdTxUpdateVariables).UpdateVariables;
			const count = Object.keys(p.variables ?? {}).length;
			return { label: `Update Variables (${count})`, mono: false };
		}
		case "DownloadOTA":
			return { label: "Download OTA", mono: false };
		case "ExtendedNetworkTest": {
			const p = (parsed.tx as CmdTxExtendedNetworkTest).ExtendedNetworkTest;
			return {
				label: `Extended Network Test (${p.duration_minutes}min)`,
				mono: false,
			};
		}
		case "StreamLogs": {
			const p = (parsed.tx as CmdTxStreamLogs).StreamLogs;
			return { label: `Stream Logs: ${p.service_name}`, mono: false };
		}
		case "StopLogStream":
			return { label: "Stop Log Stream", mono: false };
		default:
			return { label: parsed.variant, mono: false };
	}
};

// ---------------------------------------------------------------------------
// SafeCommandRx types (mirrors smithd/src/utils/schema.rs)
// ---------------------------------------------------------------------------

type CmdRxRestart = { Restart: { message: string } };
type CmdRxFreeForm = { FreeForm: { stdout: string; stderr: string } };
type CmdRxOpenTunnel = { OpenTunnel: { port_server: number } };
type CmdRxUpdateSystemInfo = { UpdateSystemInfo: { system_info: unknown } };
type CmdRxUpdatePackage = { UpdatePackage: { name: string; version: string } };
type CmdRxWifiConnect = { WifiConnect: { stdout: string; stderr: string } };
type CmdRxCheckOTAStatus = { CheckOTAStatus: { status: string } };
type CmdRxTestNetwork = {
	TestNetwork: {
		bytes_downloaded: number;
		duration_ms: number;
		bytes_uploaded?: number | null;
		upload_duration_ms?: number | null;
		timed_out: boolean;
	};
};
type CmdRxExtendedNetworkTest = {
	ExtendedNetworkTest: {
		total_duration_ms: number;
		error?: string | null;
		samples: unknown[];
		network_info?: unknown;
	};
};
type CmdRxLogStreamStarted = { LogStreamStarted: { session_id: string } };
type CmdRxLogStreamStopped = { LogStreamStopped: { session_id: string } };
type CmdRxLogStreamError = {
	LogStreamError: { session_id: string; error: string };
};

// Parse the response field: serialized SafeCommandRx directly
const parseRx = (
	response: unknown,
): { variant: string; payload: unknown } | null => {
	if (response == null) return null;
	if (typeof response === "string") return { variant: response, payload: null };
	if (typeof response !== "object") return null;
	const variant = Object.keys(response as object)[0];
	const payload = (response as Record<string, unknown>)[variant];
	return { variant, payload };
};

// ---------------------------------------------------------------------------
// Shared UI primitives
// ---------------------------------------------------------------------------

const CodeBlock = ({
	label,
	content,
	labelClassName,
}: {
	label: string;
	content: string;
	labelClassName?: string;
}) => {
	const [copied, setCopied] = useState(false);

	const handleCopy = () => {
		navigator.clipboard.writeText(content);
		setCopied(true);
		setTimeout(() => setCopied(false), 2000);
	};

	return (
		<div>
			<div className="flex items-center justify-between mb-1">
				<span
					className={`text-xs font-medium uppercase tracking-wide ${labelClassName ?? "text-gray-400"}`}
				>
					{label}
				</span>
				<button
					onClick={handleCopy}
					className="text-gray-400 hover:text-gray-600 cursor-pointer p-1 rounded"
					title={copied ? "Copied!" : "Copy"}
				>
					{copied ? (
						<Check className="w-3 h-3 text-green-500" />
					) : (
						<Copy className="w-3 h-3" />
					)}
				</button>
			</div>
			<pre className="text-xs font-mono bg-gray-900 text-gray-100 p-3 rounded overflow-x-auto whitespace-pre-wrap break-words min-h-[2.5rem]">
				{content.trim() === "" ? (
					<span className="text-gray-500 italic">(no output)</span>
				) : (
					content
				)}
			</pre>
		</div>
	);
};

// Simple key/value table for structured command params
const KVTable = ({ rows }: { rows: { key: string; value: string }[] }) => (
	<dl className="space-y-1">
		{rows.map(({ key, value }) => (
			<div key={key} className="flex gap-3 text-sm">
				<dt className="w-36 shrink-0 text-gray-400">{key}</dt>
				<dd className="text-gray-900 font-mono break-all">{value}</dd>
			</div>
		))}
	</dl>
);

// ---------------------------------------------------------------------------
// Command sent (TX) detail renderer
// ---------------------------------------------------------------------------

const renderTxDetail = (cmd_data: unknown) => {
	const parsed = parseTx(cmd_data);
	if (parsed == null)
		return <p className="text-sm text-gray-400 italic">Unknown command</p>;

	// Unit variants — no params
	const unitVariants = [
		"Ping",
		"Upgrade",
		"Restart",
		"CloseTunnel",
		"CheckOTAStatus",
		"StartOTA",
		"TestNetwork",
	];
	if (unitVariants.includes(parsed.variant)) {
		return <p className="text-sm text-gray-400 italic">No parameters</p>;
	}

	switch (parsed.variant) {
		case "FreeForm": {
			const p = (parsed.tx as CmdTxFreeForm).FreeForm;
			return (
				<CodeBlock
					label="command"
					content={p.cmd}
					labelClassName="text-blue-400"
				/>
			);
		}
		case "OpenTunnel": {
			const p = (parsed.tx as CmdTxOpenTunnel).OpenTunnel;
			const rows = [
				p.port != null ? { key: "Port", value: String(p.port) } : null,
				p.user != null ? { key: "User", value: p.user } : null,
				p.pub_key != null ? { key: "Public Key", value: p.pub_key } : null,
			].filter(Boolean) as { key: string; value: string }[];
			return rows.length > 0 ? (
				<KVTable rows={rows} />
			) : (
				<p className="text-sm text-gray-400 italic">No parameters</p>
			);
		}
		case "UpdateNetwork": {
			const net = (parsed.tx as CmdTxUpdateNetwork).UpdateNetwork.network;
			const rows = Object.entries(net)
				.filter(([, v]) => v != null)
				.map(([k, v]) => ({
					key: k,
					value: typeof v === "object" ? JSON.stringify(v) : String(v),
				}));
			return <KVTable rows={rows} />;
		}
		case "UpdateVariables": {
			const vars =
				(parsed.tx as CmdTxUpdateVariables).UpdateVariables.variables ?? {};
			const rows = Object.entries(vars).map(([k, v]) => ({ key: k, value: v }));
			return rows.length > 0 ? (
				<KVTable rows={rows} />
			) : (
				<p className="text-sm text-gray-400 italic">No variables</p>
			);
		}
		case "DownloadOTA": {
			const p = (parsed.tx as CmdTxDownloadOTA).DownloadOTA;
			return (
				<KVTable
					rows={[
						{ key: "Tools URL", value: p.tools },
						{ key: "Payload URL", value: p.payload },
						{ key: "Rate", value: String(p.rate) },
					]}
				/>
			);
		}
		case "ExtendedNetworkTest": {
			const p = (parsed.tx as CmdTxExtendedNetworkTest).ExtendedNetworkTest;
			return (
				<KVTable
					rows={[{ key: "Duration", value: `${p.duration_minutes} minutes` }]}
				/>
			);
		}
		case "StreamLogs": {
			const p = (parsed.tx as CmdTxStreamLogs).StreamLogs;
			return (
				<KVTable
					rows={[
						{ key: "Service", value: p.service_name },
						{ key: "Session ID", value: p.session_id },
					]}
				/>
			);
		}
		case "StopLogStream": {
			const p = (parsed.tx as CmdTxStopLogStream).StopLogStream;
			return <KVTable rows={[{ key: "Session ID", value: p.session_id }]} />;
		}
		default:
			return (
				<CodeBlock
					label="params"
					content={JSON.stringify(parsed.tx, null, 2)}
				/>
			);
	}
};

// ---------------------------------------------------------------------------
// Response (RX) detail renderer
// ---------------------------------------------------------------------------

const renderRxDetail = (response: unknown) => {
	const parsed = parseRx(response);
	if (parsed == null)
		return <p className="text-sm text-gray-400 italic">No response yet.</p>;

	// Unit variants
	const unitVariants: string[] = [
		"Pong",
		"GetVariables",
		"Upgraded",
		"UpdateVariables",
		"GetNetwork",
		"UpdateNetwork",
		"UpgradePackages",
		"TunnelClosed",
		"DownloadOTA",
	];
	if (unitVariants.includes(parsed.variant)) {
		return <p className="text-sm text-gray-500">{parsed.variant}</p>;
	}

	switch (parsed.variant) {
		case "FreeForm": {
			const p = parsed.payload as CmdRxFreeForm["FreeForm"];
			return (
				<div className="space-y-4">
					<CodeBlock label="stdout" content={p.stdout ?? ""} />
					{(p.stderr ?? "").trim() !== "" && (
						<CodeBlock
							label="stderr"
							content={p.stderr}
							labelClassName="text-red-400"
						/>
					)}
				</div>
			);
		}
		case "WifiConnect": {
			const p = parsed.payload as CmdRxWifiConnect["WifiConnect"];
			return (
				<div className="space-y-4">
					<CodeBlock label="stdout" content={p.stdout ?? ""} />
					{(p.stderr ?? "").trim() !== "" && (
						<CodeBlock
							label="stderr"
							content={p.stderr}
							labelClassName="text-red-400"
						/>
					)}
				</div>
			);
		}
		case "Restart": {
			const p = parsed.payload as CmdRxRestart["Restart"];
			return <CodeBlock label="output" content={p.message ?? ""} />;
		}
		case "OpenTunnel": {
			const p = parsed.payload as CmdRxOpenTunnel["OpenTunnel"];
			return (
				<KVTable
					rows={[{ key: "Server Port", value: String(p.port_server) }]}
				/>
			);
		}
		case "UpdateSystemInfo": {
			const p = parsed.payload as CmdRxUpdateSystemInfo["UpdateSystemInfo"];
			return (
				<CodeBlock
					label="system info"
					content={JSON.stringify(p.system_info, null, 2)}
				/>
			);
		}
		case "UpdatePackage": {
			const p = parsed.payload as CmdRxUpdatePackage["UpdatePackage"];
			return (
				<KVTable
					rows={[
						{ key: "Package", value: p.name },
						{ key: "Version", value: p.version },
					]}
				/>
			);
		}
		case "CheckOTAStatus": {
			const p = parsed.payload as CmdRxCheckOTAStatus["CheckOTAStatus"];
			return <KVTable rows={[{ key: "Status", value: p.status }]} />;
		}
		case "TestNetwork": {
			const p = parsed.payload as CmdRxTestNetwork["TestNetwork"];
			const dlMbps = (
				(p.bytes_downloaded * 8) /
				(p.duration_ms / 1000) /
				1_000_000
			).toFixed(2);
			const rows: { key: string; value: string }[] = [
				{
					key: "Download",
					value: `${(p.bytes_downloaded / 1024 / 1024).toFixed(2)} MB`,
				},
				{ key: "Download Speed", value: `${dlMbps} Mbps` },
				{ key: "Duration", value: `${p.duration_ms} ms` },
			];
			if (p.bytes_uploaded != null) {
				const ulMbps = (
					(p.bytes_uploaded * 8) /
					((p.upload_duration_ms ?? p.duration_ms) / 1000) /
					1_000_000
				).toFixed(2);
				rows.push({
					key: "Upload",
					value: `${(p.bytes_uploaded / 1024 / 1024).toFixed(2)} MB`,
				});
				rows.push({ key: "Upload Speed", value: `${ulMbps} Mbps` });
			}
			if (p.timed_out) rows.push({ key: "Timed Out", value: "yes" });
			return <KVTable rows={rows} />;
		}
		case "ExtendedNetworkTest": {
			const p =
				parsed.payload as CmdRxExtendedNetworkTest["ExtendedNetworkTest"];
			const rows: { key: string; value: string }[] = [
				{
					key: "Duration",
					value: `${(p.total_duration_ms / 1000).toFixed(1)}s`,
				},
				{ key: "Samples", value: String((p.samples ?? []).length) },
			];
			if (p.error) rows.push({ key: "Error", value: p.error });
			return (
				<div className="space-y-3">
					<KVTable rows={rows} />
					{p.network_info != null && (
						<CodeBlock
							label="network info"
							content={JSON.stringify(p.network_info, null, 2)}
						/>
					)}
				</div>
			);
		}
		case "LogStreamStarted": {
			const p = parsed.payload as CmdRxLogStreamStarted["LogStreamStarted"];
			return <KVTable rows={[{ key: "Session ID", value: p.session_id }]} />;
		}
		case "LogStreamStopped": {
			const p = parsed.payload as CmdRxLogStreamStopped["LogStreamStopped"];
			return <KVTable rows={[{ key: "Session ID", value: p.session_id }]} />;
		}
		case "LogStreamError": {
			const p = parsed.payload as CmdRxLogStreamError["LogStreamError"];
			return (
				<KVTable
					rows={[
						{ key: "Session ID", value: p.session_id },
						{ key: "Error", value: p.error },
					]}
				/>
			);
		}
		default:
			return (
				<CodeBlock
					label="response"
					content={JSON.stringify(parsed.payload, null, 2)}
				/>
			);
	}
};

// ---------------------------------------------------------------------------
// Status helpers
// ---------------------------------------------------------------------------

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
		<div className="space-y-6">
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
					<div
						className="flex border border-gray-200 rounded-lg overflow-hidden bg-white"
						style={{ height: "calc(100vh - 340px)" }}
					>
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
