"use client";

import { useAuth0 } from "@auth0/auth0-react";
import { ArrowDown, Check, Copy, Loader2 } from "lucide-react";
import {
	Fragment,
	memo,
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { useConfig } from "@/app/hooks/config";

interface LogViewerProps {
	deviceSerial: string;
	serviceName: string;
	showRaw?: boolean;
	onStatusChange?: (
		status: "connecting" | "connected" | "disconnected",
	) => void;
}

/** Frames the api sends on the dashboard log socket. */
type LogFrame =
	| { type: "ready" }
	| { type: "lines"; lines: string[] }
	| { type: "error"; message: string };

type Level = "error" | "warn" | "debug" | "info";

interface LogLine {
	id: number;
	date: string;
	time: string;
	label: string;
	source: string;
	message: string;
	level: Level;
	journal: string;
	continuation: boolean;
	raw: string;
}

const MAX_LINES = 5000;

/** `journalctl -o short-iso`: "<iso> <host> <unit>[<pid>]: <message>". */
const JOURNAL =
	/^(\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})(?:[+-]\d{2}:?\d{2}|Z))\s+\S+\s+.+?:\s(.*)$/;

/** Leading RFC3339 stamp some loggers prepend, e.g. tracing's default format. */
const INNER_TS =
	/^\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2}\.\d{3})\d*(?:Z|[+-]\d{2}:?\d{2}|[+-]\d{4})?\s/;

// short-iso carries no PRIORITY, so severity can only be guessed from the text.
// Match uppercase tokens only, so prose like "no errors found" stays neutral.
const ERROR_RE = /\b(ERROR|ERR|FATAL|PANIC|CRITICAL)\b|level=error|\[error\]/;
const WARN_RE = /\b(WARN|WARNING)\b|level=warn|\[warn\]/;
const DEBUG_RE = /\b(DEBUG|TRACE)\b|level=debug|\[debug\]/;
const LEVEL_PREFIX =
	/^\s*(ERROR|ERR|FATAL|PANIC|CRITICAL|WARN|WARNING|DEBUG|TRACE|INFO)(?:\s+|$)/;
const SOURCE_PREFIX = /^([\w.-]+(?:::[\w.-]+)*):\s+/;
const CONTINUATION = /^(?:\s+\S|Caused by:|Stack backtrace:)/;

const levelOf = (message: string): Level => {
	const head = LEVEL_PREFIX.exec(message)?.[1] ?? message.slice(0, 120);
	if (ERROR_RE.test(head)) return "error";
	if (WARN_RE.test(head)) return "warn";
	if (DEBUG_RE.test(head)) return "debug";
	return "info";
};

const parseLine = (raw: string, id: number, previous?: LogLine): LogLine => {
	const m = JOURNAL.exec(raw);
	let message = m ? m[3] : raw;
	let time = m ? m[2] : "";
	let date = m ? m[1].slice(0, 10) : (previous?.date ?? "");
	const journal = m ? raw.slice(0, raw.length - message.length) : "";

	// Loggers like tracing re-stamp every line, so the journal time would show
	// twice. Prefer the inner one (it has sub-second precision) and drop it.
	const inner = INNER_TS.exec(message);
	if (inner) {
		date = inner[0].slice(0, 10);
		time = inner[1];
		message = message.slice(inner[0].length);
	}

	const prefix = LEVEL_PREFIX.exec(message);
	const continuation = Boolean(
		previous &&
			!inner &&
			!prefix &&
			journal === previous.journal &&
			CONTINUATION.test(message),
	);
	const level = continuation && previous ? previous.level : levelOf(message);
	const label = continuation
		? ""
		: prefix?.[1] === "TRACE"
			? "TRACE"
			: prefix || level !== "info"
				? level.toUpperCase()
				: "";
	let source = "";
	if (prefix) {
		message = message.slice(prefix[0].length);
		const module = SOURCE_PREFIX.exec(message);
		if (module) {
			source = module[1];
			message = message.slice(module[0].length);
		}
	}

	return {
		id,
		date,
		time,
		label,
		source,
		message,
		level,
		journal,
		continuation,
		raw,
	};
};

const LEVEL_STYLES: Record<Level, string> = {
	error: "text-red-300 bg-red-500/10",
	warn: "text-amber-200",
	debug: "text-gray-500",
	info: "text-gray-100",
};

const LogRow = memo(({ line }: { line: LogLine }) => (
	<div
		className={`grid grid-cols-[12ch_5ch_minmax(0,1fr)] gap-x-3 px-3 ${LEVEL_STYLES[line.level]}`}
	>
		<span className="whitespace-nowrap tabular-nums text-gray-500">
			{!line.continuation && line.time}
		</span>
		<span className="font-medium">{line.label}</span>
		<span className="min-w-0 whitespace-pre-wrap break-words">
			{line.source && <span className="text-gray-500">{line.source}: </span>}
			{line.message}
		</span>
	</div>
));

const LogViewer = ({
	deviceSerial,
	serviceName,
	showRaw = false,
	onStatusChange,
}: LogViewerProps) => {
	const { getAccessTokenSilently } = useAuth0();
	const { config } = useConfig();
	const [logs, setLogs] = useState<LogLine[]>([]);
	// Stays true until the device actually attaches, not merely until our own
	// socket opens: the device only sees the queued command on its next poll.
	const [isConnecting, setIsConnecting] = useState(true);
	const [isConnected, setIsConnected] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [elapsed, setElapsed] = useState(0);
	const [copied, setCopied] = useState(false);
	const [pinned, setPinned] = useState(true);
	const [unseen, setUnseen] = useState(0);
	const logContainerRef = useRef<HTMLDivElement>(null);
	const wsRef = useRef<WebSocket | null>(null);
	const nextId = useRef(0);
	const previousLine = useRef<LogLine | undefined>(undefined);
	// Lines appended since the last scroll effect ran. A ref (not a length
	// delta) because the ring buffer truncates from the front: at capacity
	// `logs.length` stays flat while hundreds of lines arrive.
	const pendingLines = useRef(0);
	const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

	const jumpToBottom = useCallback(() => {
		const el = logContainerRef.current;
		if (el) el.scrollTop = el.scrollHeight;
		setPinned(true);
		setUnseen(0);
	}, []);

	const handleScroll = () => {
		const el = logContainerRef.current;
		if (!el) return;
		const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 32;
		setPinned(atBottom);
		if (atBottom) setUnseen(0);
	};

	const copyToClipboard = () => {
		navigator.clipboard
			.writeText(logs.map((l) => l.raw).join("\n"))
			.then(() => {
				setCopied(true);
				if (copyTimer.current) clearTimeout(copyTimer.current);
				copyTimer.current = setTimeout(() => setCopied(false), 2000);
			})
			.catch(() => setCopied(false));
	};

	useEffect(() => {
		return () => {
			if (copyTimer.current) clearTimeout(copyTimer.current);
		};
	}, []);

	useEffect(() => {
		if (!isConnecting) return;
		setElapsed(0);
		const started = Date.now();
		const timer = setInterval(
			() => setElapsed(Math.floor((Date.now() - started) / 1000)),
			1000,
		);
		return () => clearInterval(timer);
	}, [isConnecting]);

	useEffect(() => {
		if (!config?.API_BASE_URL) return;

		let cancelled = false;
		const connect = async () => {
			try {
				setIsConnecting(true);
				setError(null);

				const token = await getAccessTokenSilently();
				if (cancelled) return;
				if (!token) throw new Error("No access token available");
				const wsUrl = config.API_BASE_URL.replace(/^http/, "ws");
				const url = `${wsUrl}/ws/devices/${deviceSerial}/logs/${serviceName}?token=${encodeURIComponent(token)}`;

				const ws = new WebSocket(url);
				wsRef.current = ws;

				ws.onopen = () => {
					if (cancelled) {
						ws.close(1000, "Component unmounting");
						return;
					}
					setLogs([]);
					setUnseen(0);
					setPinned(true);
					pendingLines.current = 0;
					nextId.current = 0;
					previousLine.current = undefined;
				};

				ws.onmessage = (event) => {
					let frame: LogFrame;
					try {
						frame = JSON.parse(event.data);
					} catch {
						return;
					}
					if (!frame || typeof frame !== "object") return;

					switch (frame.type) {
						case "ready":
							setIsConnecting(false);
							setIsConnected(true);
							break;
						case "lines": {
							if (!Array.isArray(frame.lines)) break;
							const parsed = frame.lines
								.filter((l) => typeof l === "string")
								.map((l) => {
									const line = parseLine(
										l,
										nextId.current++,
										previousLine.current,
									);
									previousLine.current = line;
									return line;
								});
							if (parsed.length === 0) break;
							pendingLines.current += parsed.length;
							setLogs((prev) => [...prev, ...parsed].slice(-MAX_LINES));
							break;
						}
						case "error":
							setError(frame.message);
							setIsConnecting(false);
							setIsConnected(false);
							break;
					}
				};

				ws.onerror = () => {
					setError("WebSocket connection error");
					setIsConnecting(false);
					setIsConnected(false);
				};

				ws.onclose = (event) => {
					setIsConnecting(false);
					setIsConnected(false);
					if (event.code !== 1000) {
						setError(`Connection closed: ${event.reason || "Unknown reason"}`);
					}
				};
			} catch (err) {
				setError(`Failed to connect: ${err}`);
				setIsConnecting(false);
			}
		};

		connect();

		return () => {
			cancelled = true;
			if (wsRef.current) {
				wsRef.current.close(1000, "Component unmounting");
				wsRef.current = null;
			}
		};
	}, [config?.API_BASE_URL, deviceSerial, serviceName, getAccessTokenSilently]);

	// Follow the tail only while the user is already at the bottom.
	// biome-ignore lint/correctness/useExhaustiveDependencies: runs on new lines
	useLayoutEffect(() => {
		const el = logContainerRef.current;
		if (!el) return;
		const added = pendingLines.current;
		pendingLines.current = 0;
		if (pinned) el.scrollTop = el.scrollHeight;
		else if (added > 0) setUnseen((n) => n + added);
	}, [logs, showRaw]);

	useEffect(() => {
		if (onStatusChange) {
			if (isConnecting) {
				onStatusChange("connecting");
			} else if (isConnected) {
				onStatusChange("connected");
			} else {
				onStatusChange("disconnected");
			}
		}
	}, [isConnecting, isConnected, onStatusChange]);

	return (
		<div className="relative">
			{isConnecting && (
				<div className="absolute inset-0 flex items-center justify-center bg-gray-900/90 z-10">
					<div className="flex flex-col items-center gap-2">
						<Loader2 className="w-6 h-6 text-gray-400 animate-spin" />
						<span className="text-gray-400 text-sm">
							Waiting for device to connect…
						</span>
						<span className="text-gray-500 text-xs">
							Devices check in every ~20 seconds
						</span>
						<span className="text-gray-500 text-xs tabular-nums">
							{elapsed}s
						</span>
					</div>
				</div>
			)}

			<div
				ref={logContainerRef}
				onScroll={handleScroll}
				className={`bg-gray-900 py-3 font-mono text-[12px] leading-[1.45] text-gray-100 overflow-auto h-[32rem] rounded-lg ${error ? "pb-12" : ""}`}
			>
				{logs.length > 0 ? (
					logs.map((line, index) =>
						showRaw ? (
							<div
								key={line.id}
								className="min-h-[1.45em] px-3 whitespace-pre-wrap break-words"
							>
								{line.raw}
							</div>
						) : (
							<Fragment key={line.id}>
								{line.date && line.date !== logs[index - 1]?.date && (
									<div className="flex items-center gap-3 px-3 py-2 text-gray-400">
										<span className="flex-1 border-t border-gray-700" />
										<time dateTime={line.date} className="shrink-0">
											{line.date}
										</time>
										<span className="flex-1 border-t border-gray-700" />
									</div>
								)}
								<LogRow line={line} />
							</Fragment>
						),
					)
				) : (
					<div className="px-3 text-gray-500">Waiting for logs…</div>
				)}
			</div>

			{!pinned && (
				<button
					type="button"
					onClick={jumpToBottom}
					className="absolute bottom-10 left-1/2 -translate-x-1/2 flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium bg-blue-600 hover:bg-blue-500 text-white rounded-full shadow-lg transition-colors cursor-pointer"
				>
					<ArrowDown className="w-3.5 h-3.5" />
					{unseen > 0
						? `${unseen} new line${unseen === 1 ? "" : "s"}`
						: "Jump to latest"}
				</button>
			)}

			{logs.length > 0 && (
				<button
					type="button"
					onClick={copyToClipboard}
					className="absolute top-2 right-2 p-1.5 bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-white rounded opacity-60 hover:opacity-100 transition-colors cursor-pointer"
					title={copied ? "Copied!" : "Copy logs"}
				>
					{copied ? (
						<Check className="w-4 h-4 text-green-400" />
					) : (
						<Copy className="w-4 h-4" />
					)}
				</button>
			)}

			{error && (
				<div className="absolute bottom-0 inset-x-0 px-3 py-2 text-xs bg-red-950/90 text-red-200 border-t border-red-900 rounded-b-lg">
					{error}
				</div>
			)}
		</div>
	);
};

export default LogViewer;
