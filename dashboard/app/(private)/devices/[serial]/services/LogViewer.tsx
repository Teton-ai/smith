"use client";

import { useAuth0 } from "@auth0/auth0-react";
import { Check, Copy, Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useConfig } from "@/app/hooks/config";

interface LogViewerProps {
	deviceSerial: string;
	serviceName: string;
	onStatusChange?: (
		status: "connecting" | "connected" | "disconnected",
	) => void;
}

const LogViewer = ({
	deviceSerial,
	serviceName,
	onStatusChange,
}: LogViewerProps) => {
	const { getAccessTokenSilently } = useAuth0();
	const { config } = useConfig();
	const [logs, setLogs] = useState<string[]>([]);
	const [isConnecting, setIsConnecting] = useState(true);
	const [isConnected, setIsConnected] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [copied, setCopied] = useState(false);
	const logContainerRef = useRef<HTMLPreElement>(null);
	const wsRef = useRef<WebSocket | null>(null);

	const scrollToBottom = useCallback(() => {
		if (logContainerRef.current) {
			logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
		}
	}, []);

	const copyToClipboard = () => {
		navigator.clipboard.writeText(logs.join("\n"));
		setCopied(true);
		setTimeout(() => setCopied(false), 2000);
	};

	useEffect(() => {
		if (!config?.API_BASE_URL) return;

		const connect = async () => {
			try {
				setIsConnecting(true);
				setError(null);

				const token = await getAccessTokenSilently();
				const wsUrl = config.API_BASE_URL.replace(/^http/, "ws");
				const url = `${wsUrl}/ws/devices/${deviceSerial}/logs/${serviceName}?token=${token}`;

				const ws = new WebSocket(url);
				wsRef.current = ws;

				ws.onopen = () => {
					setIsConnecting(false);
					setIsConnected(true);
					setLogs([]);
				};

				ws.onmessage = (event) => {
					setLogs((prev) => [...prev, event.data]);
					scrollToBottom();
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
			if (wsRef.current) {
				wsRef.current.close(1000, "Component unmounting");
				wsRef.current = null;
			}
		};
	}, [
		config?.API_BASE_URL,
		deviceSerial,
		serviceName,
		getAccessTokenSilently,
		scrollToBottom,
	]);

	// biome-ignore lint/correctness/useExhaustiveDependencies: scroll when logs change
	useEffect(() => {
		scrollToBottom();
	}, [logs.length, scrollToBottom]);

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
		<div className="relative group">
			<div className="relative">
				{isConnecting && (
					<div className="absolute inset-0 flex items-center justify-center bg-gray-900/90 z-10">
						<div className="flex flex-col items-center gap-2">
							<Loader2 className="w-6 h-6 text-gray-400 animate-spin" />
							<span className="text-gray-400 text-sm">
								Waiting for device...
							</span>
							<span className="text-gray-500 text-xs">
								Device polls every ~20 seconds
							</span>
						</div>
					</div>
				)}

				{error && (
					<div className="absolute inset-0 flex items-center justify-center bg-gray-900/90 z-10">
						<span className="text-sm" style={{ color: "#ffffff" }}>
							{error}
						</span>
					</div>
				)}

				<pre
					ref={logContainerRef}
					className="bg-gray-900 p-3 font-mono text-xs overflow-auto h-[28rem] whitespace-pre-wrap leading-relaxed rounded-lg"
					style={{ color: "#ffffff" }}
				>
					{logs.length > 0 ? (
						logs.join("\n")
					) : (
						<span style={{ color: "#ffffff" }}>Waiting for logs...</span>
					)}
				</pre>

				{logs.length > 0 && (
					<button
						onClick={copyToClipboard}
						className="absolute top-2 right-2 p-1.5 bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-white rounded transition-colors opacity-0 group-hover:opacity-100 cursor-pointer"
						title={copied ? "Copied!" : "Copy logs"}
					>
						{copied ? (
							<Check className="w-4 h-4 text-green-400" />
						) : (
							<Copy className="w-4 h-4" />
						)}
					</button>
				)}
			</div>
		</div>
	);
};

export default LogViewer;
