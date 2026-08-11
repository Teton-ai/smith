"use client";

import { Badge } from "@teton/smith-ui";
import { Check, Copy, X } from "lucide-react";
import { type ReactNode, useState } from "react";
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
export type CmdTxGetLogs = {
	GetLogs: {
		unit?: string | null;
		since?: string | null;
		until?: string | null;
		grep?: string | null;
	};
};

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
	| CmdTxStopLogStream
	| CmdTxGetLogs;

// TX command variants that carry no parameters (unit variants).
export const SIMPLE_COMMANDS = [
	"Ping",
	"Upgrade",
	"Restart",
	"CloseTunnel",
	"CheckOTAStatus",
	"StartOTA",
	"TestNetwork",
	"WifiScan",
	"ReportNMProfiles",
	"RunAudit",
] as const;

// Curated subset of SIMPLE_COMMANDS/COMMAND_OPTIONS safe to dispatch in bulk to
// many devices at once from the devices-page modal. Deliberately excludes
// anything session-based, parameterized, or that can brick connectivity/flash
// a device (UpdateNetwork, StartOTA, DownloadOTA, OpenTunnel, StreamLogs, ...).
export const BULK_COMMAND_OPTIONS = [
	"WifiScan",
	"Ping",
	"TestNetwork",
	"ExtendedNetworkTest",
	"RunAudit",
	"ReportNMProfiles",
	"GetLogs",
	"FreeForm",
] as const;

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
export type WifiNetwork = {
	ssid: string | null;
	bssid: string;
	signal: number | null;
	rate: number | null;
	security: string | null;
	channel: number | null;
};
export type CmdRxWifiScan = { WifiScan: { networks: WifiNetwork[] } };
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
// Editable command builder (used by the recipes page and the devices bulk-
// command modal to build a SafeCommandTx from a form)
// ---------------------------------------------------------------------------

// Full set of command variants a recipe can store.
export const COMMAND_OPTIONS = [
	...SIMPLE_COMMANDS,
	"FreeForm",
	"OpenTunnel",
	"DownloadOTA",
	"ExtendedNetworkTest",
];

export type EditableCommand = {
	variant: string;
	continue_on_error: boolean;
	cmd: string;
	port: string;
	user: string;
	pub_key: string;
	tools: string;
	payload: string;
	rate: string;
	duration_minutes: string;
	unit: string;
	since: string;
	until: string;
	grep: string;
};

export const emptyCommand = (variant = "Ping"): EditableCommand => ({
	variant,
	continue_on_error: false,
	cmd: "",
	port: "",
	user: "",
	pub_key: "",
	tools: "",
	payload: "",
	rate: "",
	duration_minutes: "",
	unit: "",
	since: "",
	until: "",
	grep: "",
});

// Build the SafeCommandTx shape that smithd expects from the editable form.
export function buildCommand(ec: EditableCommand): unknown {
	switch (ec.variant) {
		case "FreeForm":
			return { FreeForm: { cmd: ec.cmd } };
		case "OpenTunnel":
			return {
				OpenTunnel: {
					port: ec.port.trim() ? Number(ec.port) : null,
					user: ec.user.trim() || null,
					pub_key: ec.pub_key.trim() || null,
				},
			};
		case "DownloadOTA":
			return {
				DownloadOTA: {
					tools: ec.tools,
					payload: ec.payload,
					rate: Number(ec.rate),
				},
			};
		case "ExtendedNetworkTest":
			return {
				ExtendedNetworkTest: {
					duration_minutes: Number(ec.duration_minutes),
				},
			};
		case "GetLogs":
			return {
				GetLogs: {
					unit: ec.unit.trim() || null,
					since: ec.since.trim() || null,
					until: ec.until.trim() || null,
					grep: ec.grep.trim() || null,
				},
			};
		default:
			return ec.variant;
	}
}

// Parse a stored command back into the editable form (for editing a recipe).
export function parseCommand(
	cmd: unknown,
	continue_on_error: boolean,
): EditableCommand {
	if (typeof cmd === "string")
		return { ...emptyCommand(cmd), continue_on_error };
	if (cmd && typeof cmd === "object") {
		const variant = Object.keys(cmd)[0];
		const p = (cmd as Record<string, Record<string, unknown>>)[variant] ?? {};
		const ec = emptyCommand(variant);
		ec.continue_on_error = continue_on_error;
		ec.cmd = (p.cmd as string) ?? "";
		ec.port = p.port != null ? String(p.port) : "";
		ec.user = (p.user as string) ?? "";
		ec.pub_key = (p.pub_key as string) ?? "";
		ec.tools = (p.tools as string) ?? "";
		ec.payload = (p.payload as string) ?? "";
		ec.rate = p.rate != null ? String(p.rate) : "";
		ec.duration_minutes =
			p.duration_minutes != null ? String(p.duration_minutes) : "";
		ec.unit = (p.unit as string) ?? "";
		ec.since = (p.since as string) ?? "";
		ec.until = (p.until as string) ?? "";
		ec.grep = (p.grep as string) ?? "";
		return ec;
	}
	return emptyCommand();
}

const isFiniteNumber = (v: string): boolean =>
	v.trim().length > 0 && Number.isFinite(Number(v));

export function commandIsValid(ec: EditableCommand): boolean {
	switch (ec.variant) {
		case "FreeForm":
			return ec.cmd.trim().length > 0;
		case "DownloadOTA":
			return (
				ec.tools.trim().length > 0 &&
				ec.payload.trim().length > 0 &&
				isFiniteNumber(ec.rate)
			);
		case "ExtendedNetworkTest":
			return (
				isFiniteNumber(ec.duration_minutes) && Number(ec.duration_minutes) > 0
			);
		default:
			return true;
	}
}

const fieldClass =
	"w-full px-3 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 placeholder-gray-400 text-sm";

// Variant picker + variant-specific inputs + continue-on-error checkbox. No
// list-row chrome (index, remove button) so it can be used standalone by
// callers that only ever hold one command, like the devices bulk-command modal.
export function CommandFields({
	command,
	options,
	onChange,
}: {
	command: EditableCommand;
	options: readonly string[];
	onChange: (next: EditableCommand) => void;
}) {
	const set = (patch: Partial<EditableCommand>) =>
		onChange({ ...command, ...patch });

	return (
		<div className="space-y-3">
			<select
				value={command.variant}
				onChange={(e) => set({ variant: e.target.value })}
				className={fieldClass}
			>
				{[...options].sort().map((opt) => (
					<option key={opt} value={opt}>
						{opt}
					</option>
				))}
			</select>

			{command.variant === "FreeForm" && (
				<input
					type="text"
					value={command.cmd}
					onChange={(e) => set({ cmd: e.target.value })}
					placeholder="e.g., ls -la /var/log"
					className={`${fieldClass} font-mono`}
				/>
			)}

			{command.variant === "OpenTunnel" && (
				<div className="grid grid-cols-3 gap-2">
					<input
						type="number"
						value={command.port}
						onChange={(e) => set({ port: e.target.value })}
						placeholder="port (optional)"
						className={fieldClass}
					/>
					<input
						type="text"
						value={command.user}
						onChange={(e) => set({ user: e.target.value })}
						placeholder="user (optional)"
						className={fieldClass}
					/>
					<input
						type="text"
						value={command.pub_key}
						onChange={(e) => set({ pub_key: e.target.value })}
						placeholder="public key (optional)"
						className={fieldClass}
					/>
				</div>
			)}

			{command.variant === "DownloadOTA" && (
				<div className="grid grid-cols-3 gap-2">
					<input
						type="text"
						value={command.tools}
						onChange={(e) => set({ tools: e.target.value })}
						placeholder="tools"
						className={fieldClass}
					/>
					<input
						type="text"
						value={command.payload}
						onChange={(e) => set({ payload: e.target.value })}
						placeholder="payload"
						className={fieldClass}
					/>
					<input
						type="number"
						step="any"
						value={command.rate}
						onChange={(e) => set({ rate: e.target.value })}
						placeholder="rate"
						className={fieldClass}
					/>
				</div>
			)}

			{command.variant === "ExtendedNetworkTest" && (
				<input
					type="number"
					value={command.duration_minutes}
					onChange={(e) => set({ duration_minutes: e.target.value })}
					placeholder="duration (minutes)"
					className={fieldClass}
				/>
			)}

			{command.variant === "GetLogs" && (
				<div className="grid grid-cols-2 gap-2">
					<input
						type="text"
						value={command.unit}
						onChange={(e) => set({ unit: e.target.value })}
						placeholder="unit (optional, e.g. smithd)"
						className={fieldClass}
					/>
					<input
						type="text"
						value={command.grep}
						onChange={(e) => set({ grep: e.target.value })}
						placeholder="grep (optional)"
						className={fieldClass}
					/>
					<input
						type="text"
						value={command.since}
						onChange={(e) => set({ since: e.target.value })}
						placeholder="since (optional, e.g. 1h ago)"
						className={fieldClass}
					/>
					<input
						type="text"
						value={command.until}
						onChange={(e) => set({ until: e.target.value })}
						placeholder="until (optional)"
						className={fieldClass}
					/>
				</div>
			)}

			<label className="flex items-center gap-2 text-xs text-gray-600">
				<input
					type="checkbox"
					checked={command.continue_on_error}
					onChange={(e) => set({ continue_on_error: e.target.checked })}
				/>
				Continue running the bundle if this command fails
			</label>
		</div>
	);
}

// One editable command inside a repeatable list (recipes page): adds the
// index label and a remove button around CommandFields.
export function CommandRow({
	command,
	index,
	onChange,
	onRemove,
}: {
	command: EditableCommand;
	index: number;
	onChange: (next: EditableCommand) => void;
	onRemove: () => void;
}) {
	return (
		<div className="border border-gray-200/80 rounded-lg p-3 space-y-3">
			<div className="flex items-start gap-2">
				<span className="text-xs font-medium text-gray-400 w-5 mt-2">
					{index + 1}.
				</span>
				<div className="flex-1">
					<CommandFields
						command={command}
						options={COMMAND_OPTIONS}
						onChange={onChange}
					/>
				</div>
				<button
					type="button"
					onClick={onRemove}
					aria-label="Remove command"
					className="text-gray-400 hover:text-red-600 cursor-pointer p-1 mt-1"
					title="Remove command"
				>
					<X className="w-4 h-4" />
				</button>
			</div>
		</div>
	);
}

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
		case "WifiScan":
			return { label: "WiFi Scan", mono: false };
		case "ReportNMProfiles":
			return { label: "Report NM Profiles", mono: false };
		case "RunAudit":
			return { label: "Run Audit", mono: false };
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
		case "GetLogs": {
			const p = (parsed.tx as CmdTxGetLogs).GetLogs;
			return { label: `Get Logs${p.unit ? `: ${p.unit}` : ""}`, mono: false };
		}
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
	meta,
}: {
	label: string;
	content: string;
	labelClassName?: string;
	/** Optional muted text shown left-aligned next to the label. */
	meta?: ReactNode;
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
					{meta && (
						<span className="ml-2 font-normal normal-case text-gray-400">
							{meta}
						</span>
					)}
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

	if ((SIMPLE_COMMANDS as readonly string[]).includes(parsed.variant)) {
		return <p className="text-sm text-gray-400 italic">No parameters</p>;
	}

	switch (parsed.variant) {
		case "FreeForm": {
			const p = (parsed.tx as CmdTxFreeForm).FreeForm;
			return <CodeBlock label="command" content={p.cmd} />;
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
		case "GetLogs": {
			const p = (parsed.tx as CmdTxGetLogs).GetLogs;
			const rows = [
				p.unit ? { key: "Unit", value: p.unit } : null,
				p.since ? { key: "Since", value: p.since } : null,
				p.until ? { key: "Until", value: p.until } : null,
				p.grep ? { key: "Grep", value: p.grep } : null,
			].filter(Boolean) as { key: string; value: string }[];
			return rows.length > 0 ? (
				<KVTable rows={rows} />
			) : (
				<p className="text-sm text-gray-400 italic">No filters</p>
			);
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
		case "WifiScan": {
			const { networks } = parsed.payload as CmdRxWifiScan["WifiScan"];
			if (networks.length === 0) {
				return (
					<p className="text-sm text-gray-400 italic">No networks found.</p>
				);
			}
			return (
				<div className="overflow-x-auto">
					<table className="min-w-full divide-y divide-gray-200">
						<thead>
							<tr>
								<th
									scope="col"
									className="text-xs font-medium text-gray-500 uppercase tracking-wider text-left pb-2"
								>
									Network
								</th>
								<th
									scope="col"
									className="text-xs font-medium text-gray-500 uppercase tracking-wider px-2 pb-2"
								>
									Channel
								</th>
								<th
									scope="col"
									className="text-xs font-medium text-gray-500 uppercase tracking-wider px-2 pb-2 text-right"
								>
									Signal
								</th>
								<th
									scope="col"
									className="text-xs font-medium text-gray-500 uppercase tracking-wider px-2 pb-2 text-right"
								>
									Rate
								</th>
								<th
									scope="col"
									className="text-xs font-medium text-gray-500 uppercase tracking-wider pl-2 pb-2"
								>
									Security
								</th>
							</tr>
						</thead>
						<tbody className="divide-y divide-gray-100">
							{networks.map((n) => (
								<tr key={`${n.bssid}-${n.channel}`}>
									<td className="py-2 pr-2 max-w-0 w-full">
										<div className="flex flex-col min-w-0">
											{n.ssid ? (
												<span className="text-sm font-medium text-gray-900 truncate">
													{n.ssid}
												</span>
											) : (
												<span className="text-sm italic text-gray-400 truncate">
													&lt;hidden&gt;
												</span>
											)}
											<span className="text-xs text-gray-500 font-mono truncate">
												{n.bssid}
											</span>
										</div>
									</td>
									<td className="px-2 py-2 text-xs text-gray-500 whitespace-nowrap">
										{n.channel ?? "—"}
									</td>
									<td className="px-2 py-2 text-xs text-gray-500 text-right whitespace-nowrap">
										{n.signal != null ? `${n.signal}%` : "—"}
									</td>
									<td className="px-2 py-2 text-xs text-gray-500 text-right whitespace-nowrap">
										{n.rate != null ? `${n.rate} Mbps` : "—"}
									</td>
									<td className="pl-2 py-2 whitespace-nowrap">
										<Badge variant="gray" pill>
											{n.security ?? "Open"}
										</Badge>
									</td>
								</tr>
							))}
						</tbody>
					</table>
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
