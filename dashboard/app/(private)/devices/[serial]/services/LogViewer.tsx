"use client";

import { useAuth0 } from "@auth0/auth0-react";
import { Check, Copy, Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useConfig } from "@/app/hooks/config";

interface LogViewerProps {
	deviceSerial: string;
	serviceName: string;
}

const LogViewer = ({ deviceSerial, serviceName }: LogViewerProps) => {
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
	}, [config?.API_BASE_URL, deviceSerial, serviceName, getAccessTokenSilently, scrollToBottom]);

	useEffect(() => {
		scrollToBottom();
	}, [logs, scrollToBottom]);

	return (
		<div className="relative">
			{isConnecting && (
				<div className="absolute inset-0 flex items-center justify-center bg-gray-900/80 rounded-lg z-10">
					<div className="flex flex-col items-center space-y-2">
						<Loader2 className="w-8 h-8 text-gray-400 animate-spin" />
						<span className="text-gray-400 text-sm">
							Connecting to device...
						</span>
						<span className="text-gray-500 text-xs">
							(Device polls every ~20 seconds)
						</span>
					</div>
				</div>
			)}

			{error && (
				<div className="absolute inset-0 flex items-center justify-center bg-gray-900/80 rounded-lg z-10">
					<div className="flex flex-col items-center space-y-2">
						<span className="text-red-400 text-sm">{error}</span>
					</div>
				</div>
			)}

			<div className="relative group">
				<div className="flex items-center justify-between mb-2">
					<div className="flex items-center space-x-2">
						<div
							className={`w-2 h-2 rounded-full ${
								isConnected ? "bg-green-500" : "bg-gray-500"
							}`}
						/>
						<span className="text-xs text-gray-500">
							{isConnected ? "Connected" : "Disconnected"}
						</span>
					</div>
					{logs.length > 0 && (
						<span className="text-xs text-gray-500">
							{logs.length} lines
						</span>
					)}
				</div>

				<pre
					ref={logContainerRef}
					className="bg-gray-900 text-gray-100 p-4 rounded-lg font-mono text-xs overflow-auto h-96 whitespace-pre-wrap"
				>
					{logs.length > 0 ? logs.join("\n") : "Waiting for logs..."}
				</pre>

				{logs.length > 0 && (
					<button
						onClick={copyToClipboard}
						className="absolute top-10 right-2 p-2 text-gray-400 hover:text-white hover:bg-gray-700 rounded transition-all opacity-0 group-hover:opacity-100"
						title={copied ? "Copied!" : "Copy to clipboard"}
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
