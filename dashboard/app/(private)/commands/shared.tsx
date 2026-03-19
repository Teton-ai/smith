"use client";

import { Check, Copy } from "lucide-react";
import { useState } from "react";
import type { DeviceCommandResponse } from "@/app/api-client";

// ---------------------------------------------------------------------------
// SafeCommandTx types (mirrors smithd/src/utils/schema.rs)
// ---------------------------------------------------------------------------

export type CmdTxPing = "Ping";
export type CmdTxUpgrade = "Upgrade";
export type CmdTxRestart = "Restart";
export type CmdTxCloseTunnel = "CloseTunnel";
export type CmdTxCheckOTAStatus = "CheckOTAStatus";
export type CmdTxStartOTA = "StartOTA";
export type CmdTxTestNetwork = "TestNetwork";

export type CmdTxFreeForm = { FreeForm: { cmd: string } };
export type CmdTxOpenTunnel = {
	OpenTunnel: {
		port?: number | null;
		user?: string | null;
		pub_key?: string | null;
	};
};
export type CmdTxUpdateNetwork = {
	UpdateNetwork: {
		network: { name?: string; network_type?: string; [k: string]: unknown };
	};
};
export type CmdTxUpdateVariables = {
	UpdateVariables: { variables: Record<string, string> };
};
export type CmdTxDownloadOTA = {
	DownloadOTA: { tools: string; payload: string; rate: number };
};
export type CmdTxExtendedNetworkTest = {
	ExtendedNetworkTest: { duration_minutes: number };
};
export type CmdTxStreamLogs = {
	StreamLogs: { session_id: string; service_name: string };
};
export type CmdTxStopLogStream = { StopLogStream: { session_id: string } };

export type SafeCommandTx =
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

// ---------------------------------------------------------------------------
// SafeCommandRx types (mirrors smithd/src/utils/schema.rs)
// ---------------------------------------------------------------------------

export type CmdRxRestart = { Restart: { message: string } };
export type CmdRxFreeForm = { FreeForm: { stdout: string; stderr: string } };
export type CmdRxOpenTunnel = { OpenTunnel: { port_server: number } };
export type CmdRxUpdateSystemInfo = {
	UpdateSystemInfo: { system_info: unknown };
};
export type CmdRxUpdatePackage = {
	UpdatePackage: { name: string; version: string };
};
export type CmdRxWifiConnect = {
	WifiConnect: { stdout: string; stderr: string };
};
export type CmdRxCheckOTAStatus = { CheckOTAStatus: { status: string } };
export type CmdRxTestNetwork = {
	TestNetwork: {
		bytes_downloaded: number;
		duration_ms: number;
		bytes_uploaded?: number | null;
		upload_duration_ms?: number | null;
		timed_out: boolean;
	};
};
export type CmdRxExtendedNetworkTest = {
	ExtendedNetworkTest: {
		total_duration_ms: number;
		error?: string | null;
		samples: unknown[];
		network_info?: unknown;
	};
};
export type CmdRxLogStreamStarted = {
	LogStreamStarted: { session_id: string };
};
export type CmdRxLogStreamStopped = {
	LogStreamStopped: { session_id: string };
};
export type CmdRxLogStreamError = {
	LogStreamError: { session_id: string; error: string };
};

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

export const parseTx = (
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

export const parseRx = (
	response: unknown,
): { variant: string; payload: unknown } | null => {
	if (response == null) return null;
	if (typeof response === "string") return { variant: response, payload: null };
	if (typeof response !== "object") return null;
	const variant = Object.keys(response as object)[0];
	const payload = (response as Record<string, unknown>)[variant];
	return { variant, payload };
};

// Short label for command list items
export const getTxLabel = (
	cmd_data: unknown,
): { label: string; mono: boolean } => {
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
// Status helpers
// ---------------------------------------------------------------------------

export const getCommandStatus = (cmd: DeviceCommandResponse) => {
	if (cmd.cancelled) return "cancelled";
	if (!cmd.fetched) return "pending";
	if (!cmd.response_at) return "executing";
	return cmd.status === 0 ? "success" : "failed";
};

export const getStatusColor = (status: string) => {
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
// UI primitives
// ---------------------------------------------------------------------------

export const CodeBlock = ({
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

export const KVTable = ({
	rows,
}: {
	rows: { key: string; value: string }[];
}) => (
	<dl className="space-y-2">
		{rows.map(({ key, value }) => (
			<div key={key} className="text-sm">
				<dt className="text-gray-400 break-all">{key}</dt>
				<dd className="text-gray-900 font-mono break-all pl-2">{value}</dd>
			</div>
		))}
	</dl>
);

// ---------------------------------------------------------------------------
// TX detail renderer
// ---------------------------------------------------------------------------

export const renderTxDetail = (cmd_data: unknown) => {
	const parsed = parseTx(cmd_data);
	if (parsed == null)
		return <p className="text-sm text-gray-400 italic">Unknown command</p>;

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
// RX detail renderer
// ---------------------------------------------------------------------------

export const renderRxDetail = (response: unknown) => {
	const parsed = parseRx(response);
	if (parsed == null)
		return <p className="text-sm text-gray-400 italic">No response yet.</p>;

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
