import { useAuth0 } from "@auth0/auth0-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useConfig } from "@/app/hooks/config";

export type FileKind = "File" | "Dir" | "Symlink" | "Other";

export interface DirEntry {
	name: string;
	kind: FileKind;
	size: number;
	mtime: number | null;
	mode: number;
	uid: number;
	gid: number;
	symlink_target: string | null;
	reachable: boolean;
}

export interface Listing {
	path: string;
	entries: DirEntry[];
	truncated: boolean;
}

export type FileOpErrorCode =
	| "NotFound"
	| "PermissionDenied"
	| "NotADirectory"
	| "NotRegularFile"
	| "TooLarge"
	| "TooManyOpenFiles"
	| "Io"
	| "Timeout";

export class FileOpError extends Error {
	code: FileOpErrorCode;

	constructor(code: FileOpErrorCode, message: string) {
		super(message);
		this.code = code;
		this.name = "FileOpError";
	}
}

export type SessionStatus = "connecting" | "ready" | "error" | "closed";

interface DownloadReady {
	url: string;
	name: string;
	size: number;
}

type Pending =
	| { kind: "list"; resolve: (l: Listing) => void; reject: (e: Error) => void }
	| {
			kind: "download";
			resolve: (d: DownloadReady) => void;
			reject: (e: Error) => void;
	  };

/**
 * Owns the file-browsing websocket and turns its frames back into promises, so
 * callers can `await list(path)` instead of wiring up their own correlation.
 *
 * The device is not persistently connected: the api queues a command that the
 * device only sees on its next poll (~20s when idle), so the handshake is slow
 * once and then every operation is a normal round trip. `elapsed` exists so the
 * UI can show that the wait is progressing rather than stuck.
 */
export function useFileSession(deviceSerial: string) {
	const { getAccessTokenSilently } = useAuth0();
	const { config } = useConfig();

	const [status, setStatus] = useState<SessionStatus>("connecting");
	const [error, setError] = useState<string | null>(null);
	const [elapsed, setElapsed] = useState(0);
	const [attempt, setAttempt] = useState(0);

	const wsRef = useRef<WebSocket | null>(null);
	const pendingRef = useRef(new Map<number, Pending>());
	const opIdRef = useRef(0);

	useEffect(() => {
		if (status !== "connecting") return;
		setElapsed(0);
		const started = Date.now();
		const timer = setInterval(
			() => setElapsed(Math.floor((Date.now() - started) / 1000)),
			1000,
		);
		return () => clearInterval(timer);
	}, [status]);

	// biome-ignore lint/correctness/useExhaustiveDependencies: `attempt` is the reconnect trigger
	useEffect(() => {
		if (!config?.API_BASE_URL) return;

		let disposed = false;
		const pending = pendingRef.current;

		const failAll = (message: string) => {
			for (const entry of pending.values()) {
				entry.reject(new FileOpError("Io", message));
			}
			pending.clear();
		};

		const connect = async () => {
			try {
				const token = await getAccessTokenSilently();
				if (disposed) return;

				const wsUrl = config.API_BASE_URL.replace(/^http/, "ws");
				const ws = new WebSocket(
					`${wsUrl}/ws/devices/${deviceSerial}/files?token=${token}`,
				);
				wsRef.current = ws;

				ws.onmessage = (event) => {
					let frame: Record<string, unknown>;
					try {
						frame = JSON.parse(event.data);
					} catch {
						return;
					}
					handleFrame(frame, pending, setStatus, setError);
				};

				ws.onerror = () => {
					if (disposed) return;
					setStatus("error");
					setError("Connection error");
					failAll("Connection error");
				};

				ws.onclose = (event) => {
					if (disposed) return;
					setStatus((prev) => (prev === "error" ? prev : "closed"));
					if (event.code !== 1000 && event.code !== 1005) {
						setError((prev) => prev ?? "Connection closed unexpectedly");
					}
					failAll("Session closed");
				};
			} catch (err) {
				if (disposed) return;
				setStatus("error");
				setError(`Failed to connect: ${err}`);
			}
		};

		connect();

		return () => {
			disposed = true;
			failAll("Session closed");
			wsRef.current?.close(1000, "Component unmounting");
			wsRef.current = null;
		};
	}, [config?.API_BASE_URL, deviceSerial, getAccessTokenSilently, attempt]);

	const send = useCallback(
		<T>(kind: Pending["kind"], body: Record<string, unknown>): Promise<T> =>
			new Promise<T>((resolve, reject) => {
				const ws = wsRef.current;
				if (!ws || ws.readyState !== WebSocket.OPEN) {
					reject(new FileOpError("Io", "Not connected to the device"));
					return;
				}

				const opId = ++opIdRef.current;
				pendingRef.current.set(opId, {
					kind,
					resolve,
					reject,
				} as unknown as Pending);
				ws.send(JSON.stringify({ ...body, op_id: opId }));
			}),
		[],
	);

	const list = useCallback(
		(path: string) => send<Listing>("list", { type: "list", path }),
		[send],
	);

	const download = useCallback(
		(path: string) =>
			send<DownloadReady>("download", { type: "download", path }),
		[send],
	);

	const retry = useCallback(() => {
		setStatus("connecting");
		setError(null);
		setAttempt((n) => n + 1);
	}, []);

	return { status, error, elapsed, list, download, retry };
}

function handleFrame(
	frame: Record<string, unknown>,
	pending: Map<number, Pending>,
	setStatus: (s: SessionStatus) => void,
	setError: (e: string | null) => void,
) {
	// Frames arrive in two shapes: control frames the api generates, tagged
	// with `type`, and relayed FileOpResponse variants, which serde encodes as
	// a single-key object.
	if (frame.type === "ready") {
		setStatus("ready");
		setError(null);
		return;
	}

	if (frame.type === "download_ready") {
		const opId = Number(frame.op_id);
		const entry = pending.get(opId);
		pending.delete(opId);
		if (entry?.kind === "download") {
			entry.resolve({
				url: String(frame.url),
				name: String(frame.name),
				size: Number(frame.size),
			});
		}
		return;
	}

	if (frame.type === "error") {
		// A session-level error, not tied to one operation.
		setStatus("error");
		setError(String(frame.message ?? "Session error"));
		return;
	}

	const listing = frame.Listing as
		| { op_id: number; path: string; entries: DirEntry[]; truncated: boolean }
		| undefined;
	if (listing) {
		const entry = pending.get(listing.op_id);
		pending.delete(listing.op_id);
		if (entry?.kind === "list") {
			entry.resolve({
				path: listing.path,
				entries: listing.entries,
				truncated: listing.truncated,
			});
		}
		return;
	}

	const failure = frame.Error as
		| { op_id: number; code: FileOpErrorCode; message: string }
		| undefined;
	if (failure) {
		const entry = pending.get(failure.op_id);
		pending.delete(failure.op_id);
		entry?.reject(new FileOpError(failure.code, failure.message));
	}

	// `Opened` and `UploadFinished` are intermediate steps of a download; the
	// browser only cares about `download_ready`, which the api sends once the
	// bytes are actually staged.
}
