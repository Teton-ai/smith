"use client";

import { Button } from "@teton/smith-ui";
import {
	ArrowLeft,
	BarChart3,
	Check,
	Copy,
	Cpu,
	ExternalLink,
	GitBranch,
	Power,
	Router,
	ScrollText,
	Signal,
	Tag,
	Terminal,
	Wifi,
} from "lucide-react";
import { useRef, useState } from "react";
import { Link } from "react-router";
import {
	type CommandRecipe,
	type Device,
	useGetRecipes,
	useIssueCommandsToDevices,
	useTriggerRecipe,
} from "@/app/api-client";
import { Modal } from "@/app/components/modal";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import { useCommandPalette } from "@/app/hooks/commandPalette";
import { useConfig } from "@/app/hooks/config";

const Tooltip = ({
	children,
	content,
}: {
	children: React.ReactNode;
	content: string;
}) => {
	const [isVisible, setIsVisible] = useState(false);
	const [position, setPosition] = useState<"top" | "right" | "left">("top");
	const containerRef = useRef<HTMLDivElement>(null);

	const handleMouseEnter = () => {
		setIsVisible(true);
		if (containerRef.current) {
			const rect = containerRef.current.getBoundingClientRect();
			const viewportWidth = window.innerWidth;

			// Estimate tooltip width (adjust based on content length)
			const estimatedTooltipWidth = content.length * 8 + 32; // rough estimate

			// If tooltip would be cut off on the right side, position it to the left
			if (rect.right + estimatedTooltipWidth > viewportWidth - 20) {
				// 20px buffer
				setPosition("left");
			} else if (rect.left < 150) {
				setPosition("right");
			} else {
				setPosition("top");
			}
		}
	};

	return (
		<div
			ref={containerRef}
			className="relative inline-block"
			onMouseEnter={handleMouseEnter}
			onMouseLeave={() => setIsVisible(false)}
		>
			{children}
			{isVisible &&
				(position === "top" ? (
					<div className="absolute bottom-full left-1/2 transform -translate-x-1/2 mb-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50 pointer-events-none">
						{content}
						<div className="absolute top-full left-1/2 transform -translate-x-1/2 w-0 h-0 border-l-4 border-r-4 border-t-4 border-l-transparent border-r-transparent border-t-gray-800"></div>
					</div>
				) : position === "right" ? (
					<div className="absolute left-full top-1/2 transform -translate-y-1/2 ml-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50 pointer-events-none">
						{content}
						<div className="absolute right-full top-1/2 transform -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-r-4 border-t-transparent border-b-transparent border-r-gray-800"></div>
					</div>
				) : (
					<div className="absolute right-full top-1/2 transform -translate-y-1/2 mr-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50 pointer-events-none">
						{content}
						<div className="absolute left-full top-1/2 transform -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-l-4 border-t-transparent border-b-transparent border-l-gray-800"></div>
					</div>
				))}
		</div>
	);
};

interface DeviceHeaderProps {
	device?: Device;
	serial: string;
	back: string;
}

/** Doubles as the device icon and the "back to devices" link: on hover it
 *  morphs from the Cpu glyph into a back arrow. */
const DeviceIconBackLink = ({ back }: { back: string }) => (
	<Link
		to={back}
		title="Back to devices"
		className="group relative flex-shrink-0 w-9 h-9 flex items-center justify-center bg-gray-100 hover:bg-gray-200 text-gray-600 rounded transition-colors"
	>
		<Cpu className="w-5 h-5 transition-all duration-200 group-hover:opacity-0 group-hover:-translate-x-1" />
		<ArrowLeft className="w-5 h-5 absolute opacity-0 translate-x-1 transition-all duration-200 group-hover:opacity-100 group-hover:translate-x-0" />
	</Link>
);

const DeviceHeader: React.FC<DeviceHeaderProps> = ({ device, serial, back }) => {
	const [sshCopied, setSshCopied] = useState(false);
	const [showRunModal, setShowRunModal] = useState(false);
	const [showRebootModal, setShowRebootModal] = useState(false);
	const [showRecipeModal, setShowRecipeModal] = useState(false);
	const [selectedRecipeId, setSelectedRecipeId] = useState<number | null>(null);
	const [recipeTriggered, setRecipeTriggered] = useState(false);
	const [recipeError, setRecipeError] = useState<string | null>(null);
	const [runCommand, setRunCommand] = useState("");
	const { config } = useConfig();
	const openCommandPalette = useCommandPalette();

	const { data: recipesData, isLoading: isLoadingRecipes } = useGetRecipes();
	const recipes: CommandRecipe[] = Array.isArray(recipesData)
		? (recipesData as CommandRecipe[])
		: [];

	const { mutate: triggerRecipe, isPending: isTriggeringRecipe } =
		useTriggerRecipe();

	const closeRecipeModal = () => {
		if (isTriggeringRecipe) return;
		setShowRecipeModal(false);
		setSelectedRecipeId(null);
		setRecipeTriggered(false);
		setRecipeError(null);
	};

	const handleTriggerRecipe = () => {
		if (selectedRecipeId == null || !device?.id) return;
		// The API loads the recipe's commands server-side and gates on
		// `recipes:trigger`, so we only send the recipe id and target device.
		setRecipeError(null);
		triggerRecipe(
			{ recipeId: selectedRecipeId, data: { devices: [device.id] } },
			{
				onSuccess: () => {
					setRecipeError(null);
					setRecipeTriggered(true);
				},
				onError: () =>
					setRecipeError("Failed to trigger recipe. Please try again."),
			},
		);
	};

	const { mutate: issueCommands, isPending: isIssuingCommands } =
		useIssueCommandsToDevices({
			mutation: {
				onSuccess: () => {
					setShowRunModal(false);
					setRunCommand("");
				},
				onError: (error) => {
					console.error("Failed to issue command:", error);
				},
			},
		});

	const { mutate: issueReboot, isPending: isRebooting } =
		useIssueCommandsToDevices({
			mutation: {
				onSuccess: () => {
					setShowRebootModal(false);
				},
				onError: (error) => {
					console.error("Failed to reboot device:", error);
				},
			},
		});

	const handleRunCommand = () => {
		if (!runCommand.trim() || !device?.id) return;
		issueCommands({
			data: {
				devices: [device.id],
				commands: [
					{
						id: -1,
						command: { FreeForm: { cmd: runCommand } },
						continue_on_error: false,
					},
				],
			},
		});
	};

	const handleReboot = () => {
		if (!device?.id) return;
		issueReboot({
			data: {
				devices: [device.id],
				commands: [
					{
						id: -1,
						command: "Restart",
						continue_on_error: false,
					},
				],
			},
		});
	};

	const getGrafanaUrl = () => {
		if (!config?.DEVICE_GRAFANA_URL) return null;
		return config.DEVICE_GRAFANA_URL.replace(
			"{serial_number}",
			device?.serial_number || serial,
		);
	};

	const handleSshTunnel = async () => {
		const command = `sm tunnel ${device?.serial_number || serial}`;
		try {
			await navigator.clipboard.writeText(command);
			setSshCopied(true);
			setTimeout(() => setSshCopied(false), 2000);
		} catch (err) {
			console.error("Failed to copy to clipboard:", err);
		}
	};
	if (!device) {
		return (
			<div className="bg-white rounded-lg border border-gray-200 p-4">
				<div className="flex items-center space-x-3">
					<DeviceIconBackLink back={back} />
					<div className="flex-1">
						<div className="flex items-center space-x-3">
							<h1
								className="text-xl font-bold text-gray-900 hover:text-blue-600 cursor-pointer transition-colors"
								onClick={openCommandPalette}
								title="Search devices"
							>
								{serial}
							</h1>
						</div>
						<div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
							<span>Loading...</span>
						</div>
					</div>
				</div>
			</div>
		);
	}

	const formatTimeAgo = (date: string) => {
		const now = new Date();
		const past = new Date(date);
		const diff = now.getTime() - past.getTime();
		const minutes = Math.floor(diff / 60000);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days}d ago`;
		if (hours > 0) return `${hours}h ago`;
		return `${minutes}m ago`;
	};

	const getDeviceStatus = () => {
		if (!device || !device.last_seen) return "offline";

		const lastSeen = new Date(device.last_seen);
		const now = new Date();
		const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);

		return diffMinutes <= 3 ? "online" : "offline";
	};

	const hasUpdatePending = () => {
		return (
			device?.release_id &&
			device.target_release_id &&
			device.release_id !== device.target_release_id
		);
	};

	const getUpdateStatus = (): {
		status: "updating" | "outdated";
		duration: string;
	} | null => {
		if (!hasUpdatePending()) return null;

		if (!device?.target_release_id_set_at)
			return { status: "outdated", duration: "" }; // No timestamp = legacy

		const setAt = new Date(device.target_release_id_set_at);
		const now = new Date();
		const diffMinutes = Math.floor(
			(now.getTime() - setAt.getTime()) / (1000 * 60),
		);
		const diffHours = Math.floor(diffMinutes / 60);
		const diffDays = Math.floor(diffHours / 24);

		let duration: string;
		if (diffDays > 0) {
			duration = `${diffDays}d`;
		} else if (diffHours > 0) {
			duration = `${diffHours}h`;
		} else {
			duration = `${diffMinutes}m`;
		}

		return {
			status: diffMinutes < 30 ? "updating" : "outdated",
			duration,
		};
	};

	const getNetworkQualityTooltip = () => {
		const status = getDeviceStatus();
		const networkScore = device?.network?.network_score;
		const downloadSpeed = device?.network?.download_speed_mbps;
		const uploadSpeed = device?.network?.upload_speed_mbps;
		const lastSeenText = device?.last_seen
			? formatTimeAgo(device.last_seen)
			: "Never";

		if (status === "offline") {
			return `Offline\nLast seen: ${lastSeenText}`;
		}

		if (!networkScore) {
			return `Online\nLast seen: ${lastSeenText}`;
		}

		const qualityText =
			networkScore >= 4 ? "Excellent" : networkScore === 3 ? "Good" : "Poor";
		const downloadText = downloadSpeed
			? `↓ ${downloadSpeed.toFixed(1)} Mbps`
			: "";
		const uploadText = uploadSpeed ? `↑ ${uploadSpeed.toFixed(1)} Mbps` : "";
		const speedText =
			downloadText || uploadText
				? ` (${[downloadText, uploadText].filter(Boolean).join(" / ")})`
				: "";
		const lastTested = device?.network?.updated_at
			? formatTimeAgo(device.network.updated_at)
			: "never";

		return `Online - ${qualityText} Network (${networkScore}/5)${speedText}\nLast tested: ${lastTested}\nLast seen: ${lastSeenText}`;
	};

	const getPrimaryConnectionType = () => {
		if (!device) return null;

		// If device has a modem, prioritize cellular
		if (device.modem_id && device.modem) {
			return "cellular";
		}

		// Check for active network connections
		const connectedInterfaces = device.system_info?.connection_statuses?.filter(
			(conn) => conn.connection_state === "connected",
		);

		if (!connectedInterfaces || connectedInterfaces.length === 0) {
			return null;
		}

		// Prioritize: WiFi > Ethernet > Other
		if (connectedInterfaces.some((conn) => conn.device_type === "wifi")) {
			return "wifi";
		}

		if (connectedInterfaces.some((conn) => conn.device_type === "ethernet")) {
			return "ethernet";
		}

		return "other";
	};

	const getConnectionIcon = (connectionType: string | null) => {
		switch (connectionType) {
			case "cellular":
				return <Signal className="w-4 h-4 text-blue-600" />;
			case "wifi":
				return <Wifi className="w-4 h-4 text-green-600" />;
			case "ethernet":
				return <Router className="w-4 h-4 text-orange-600" />;
			default:
				return null;
		}
	};

	const getConnectionTooltip = (connectionType: string | null) => {
		if (!device) return "";

		switch (connectionType) {
			case "cellular":
				return `Cellular Connection${device.modem?.network_provider ? ` - ${device.modem.network_provider}` : ""}${device.modem ? `\nIMEI: ${device.modem.imei}` : ""}${device.modem?.on_dongle ? "\nExternal Dongle" : "\nBuilt-in Modem"}`;
			case "wifi": {
				const wifiConnections = device.system_info?.connection_statuses?.filter(
					(conn) =>
						conn.connection_state === "connected" &&
						conn.device_type === "wifi",
				);
				const primaryWifi = wifiConnections?.[0];
				return `WiFi Connection${primaryWifi?.connection_name ? ` - ${primaryWifi.connection_name}` : ""}`;
			}
			case "ethernet": {
				const ethConnections = device.system_info?.connection_statuses?.filter(
					(conn) =>
						conn.connection_state === "connected" &&
						conn.device_type === "ethernet",
				);
				return `Ethernet Connection${ethConnections ? ` - ${ethConnections.length} interface(s)` : ""}`;
			}
			default:
				return "No active connection detected";
		}
	};

	const getDeviceName = () => device?.serial_number || serial;

	const status = getDeviceStatus();
	const connectionType = getPrimaryConnectionType();

	return (
		<div className="bg-white rounded-lg border border-gray-200 p-4">
			<div className="flex flex-col gap-3 lg:flex-row lg:items-center">
				<div className="flex flex-1 items-center space-x-3 min-w-0">
					<DeviceIconBackLink back={back} />
					<div className="flex-1 min-w-0">
						<div className="flex items-center space-x-3">
							<h1
								className="text-xl font-bold text-gray-900 hover:text-blue-600 cursor-pointer transition-colors"
								onClick={openCommandPalette}
								title="Search devices"
							>
								{getDeviceName()}
							</h1>
							<Tooltip content={getNetworkQualityTooltip()}>
								<div className="flex-shrink-0 cursor-help">
									<NetworkQualityIndicator
										isOnline={status === "online"}
										networkScore={device?.network?.network_score}
									/>
								</div>
							</Tooltip>
							{getUpdateStatus()?.status === "updating" && (
								<Tooltip
									content={`Updating for ${getUpdateStatus()?.duration}: ${device.release?.distribution_name}@${device.release?.version || device.release_id} → ${device.target_release?.distribution_name}@${device.target_release?.version || device.target_release_id}`}
								>
									<span className="px-2 py-1 text-xs font-medium bg-blue-100 text-blue-800 rounded-full cursor-help">
										Updating {getUpdateStatus()?.duration}
									</span>
								</Tooltip>
							)}
							{getUpdateStatus()?.status === "outdated" && (
								<Tooltip
									content={`Update failed after ${getUpdateStatus()?.duration}: ${device.release?.distribution_name}@${device.release?.version || device.release_id} → ${device.target_release?.distribution_name}@${device.target_release?.version || device.target_release_id}`}
								>
									<span className="px-2 py-1 text-xs font-medium bg-orange-100 text-orange-800 rounded-full cursor-help">
										Update Failed {getUpdateStatus()?.duration}
									</span>
								</Tooltip>
							)}
						</div>
						<div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
							{device.release && (
								<div className="flex items-center space-x-2">
									<div className="flex items-center space-x-1">
										<GitBranch className="w-4 h-4" />
										<span className="font-medium">
											{device.release.distribution_name}
										</span>
									</div>
									<div className="flex items-center space-x-1">
										<Tag className="w-4 h-4" />
										<span>v{device.release.version}</span>
									</div>
								</div>
							)}
							{connectionType && (
								<Tooltip content={getConnectionTooltip(connectionType)}>
									<div className="flex items-center space-x-1 cursor-help">
										{getConnectionIcon(connectionType)}
										<span className="capitalize">{connectionType}</span>
									</div>
								</Tooltip>
							)}
						</div>
					</div>
				</div>
				<div className="flex flex-wrap items-center gap-2 lg:flex-shrink-0 lg:gap-3">
					<Tooltip content="Run a command on this device">
						<Button
							variant="soft"
							tone="purple"
							icon={<Terminal className="w-4 h-4" />}
							onClick={() => setShowRunModal(true)}
						>
							Run
						</Button>
					</Tooltip>
					<Tooltip content="Apply a recipe to this device">
						<Button
							variant="soft"
							tone="purple"
							icon={<ScrollText className="w-4 h-4" />}
							onClick={() => setShowRecipeModal(true)}
						>
							Recipe
						</Button>
					</Tooltip>
					<Tooltip content="Reboot this device">
						<Button
							variant="soft"
							tone="red"
							icon={<Power className="w-4 h-4" />}
							onClick={() => setShowRebootModal(true)}
						>
							Reboot
						</Button>
					</Tooltip>
					<Tooltip
						content={
							sshCopied
								? "Copied to clipboard!"
								: `Copy SSH tunnel command: sm tunnel ${device?.serial_number || serial}`
						}
					>
						<Button
							variant="soft"
							tone={sshCopied ? "green" : "blue"}
							onClick={handleSshTunnel}
							icon={
								sshCopied ? (
									<Check className="w-4 h-4" />
								) : (
									<>
										<Terminal className="w-4 h-4" />
										<Copy className="w-3 h-3" />
									</>
								)
							}
						>
							{sshCopied ? "Copied!" : "SSH"}
						</Button>
					</Tooltip>
					{getGrafanaUrl() && (
						<Tooltip content="Open Grafana dashboard">
							<Button
								variant="soft"
								tone="orange"
								href={getGrafanaUrl()!}
								target="_blank"
								icon={
									<>
										<BarChart3 className="w-4 h-4" />
										<ExternalLink className="w-3 h-3" />
									</>
								}
							>
								Grafana
							</Button>
						</Tooltip>
					)}
				</div>
			</div>

			{/* Run Command Modal */}
			<Modal
				open={showRunModal}
				onClose={() => {
					setShowRunModal(false);
					setRunCommand("");
				}}
				title="Run Command"
				footer={
					<>
						<Button
							variant="soft"
							tone="gray"
							disabled={isIssuingCommands}
							onClick={() => {
								setShowRunModal(false);
								setRunCommand("");
							}}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="purple"
							loading={isIssuingCommands}
							disabled={!runCommand.trim()}
							onClick={handleRunCommand}
						>
							{isIssuingCommands ? "Sending..." : "Run Command"}
						</Button>
					</>
				}
			>
				<div className="bg-purple-50 border border-purple-200 rounded-lg p-4 mb-4">
					<div className="flex gap-3">
						<Terminal className="w-5 h-5 text-purple-600 flex-shrink-0 mt-0.5" />
						<div>
							<p className="text-purple-800 font-medium">
								Execute Command on {getDeviceName()}
							</p>
							<p className="text-purple-700 text-sm mt-1">
								The command will be queued and executed on the device when it
								checks in.
							</p>
						</div>
					</div>
				</div>

				<div>
					<label className="block text-sm font-medium text-gray-700 mb-2">
						Command
					</label>
					<input
						type="text"
						value={runCommand}
						onChange={(e) => setRunCommand(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter" && runCommand.trim()) {
								handleRunCommand();
							}
						}}
						placeholder="e.g., ls -la /var/log"
						className="w-full px-3 py-2 border border-gray-300 rounded-md focus:ring-purple-500 focus:border-purple-500 text-gray-900 placeholder-gray-400"
					/>
					<p className="mt-1 text-xs text-gray-500">
						Enter a shell command to execute on this device
					</p>
				</div>
			</Modal>

			{/* Reboot Confirmation Modal */}
			<Modal
				open={showRebootModal}
				onClose={() => setShowRebootModal(false)}
				title="Reboot Device"
				footer={
					<>
						<Button
							variant="soft"
							tone="gray"
							disabled={isRebooting}
							onClick={() => setShowRebootModal(false)}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="red"
							loading={isRebooting}
							onClick={handleReboot}
						>
							{isRebooting ? "Rebooting..." : "Reboot Device"}
						</Button>
					</>
				}
			>
				<div className="bg-red-50 border border-red-200 rounded-lg p-4">
					<div className="flex gap-3">
						<Power className="w-5 h-5 text-red-600 flex-shrink-0 mt-0.5" />
						<div>
							<p className="text-red-800 font-medium">
								Reboot {getDeviceName()}
							</p>
							<p className="text-red-700 text-sm mt-1">
								This will restart the device. It will be temporarily offline
								until it comes back up.
							</p>
						</div>
					</div>
				</div>
			</Modal>

			{/* Apply Recipe Modal */}
			<Modal
				open={showRecipeModal}
				onClose={closeRecipeModal}
				title="Apply Recipe"
				subtitle={`Trigger a recipe onto ${getDeviceName()}`}
				footer={
					recipeTriggered ? (
						<Button variant="solid" tone="blue" onClick={closeRecipeModal}>
							Done
						</Button>
					) : (
						<>
							<Button
								variant="soft"
								tone="gray"
								onClick={closeRecipeModal}
								disabled={isTriggeringRecipe}
							>
								Cancel
							</Button>
							<Button
								variant="solid"
								tone="purple"
								loading={isTriggeringRecipe}
								disabled={selectedRecipeId == null}
								onClick={handleTriggerRecipe}
							>
								{isTriggeringRecipe ? "Triggering..." : "Trigger Recipe"}
							</Button>
						</>
					)
				}
			>
				{recipeTriggered ? (
					<div className="bg-green-50 border border-green-200 text-green-800 text-sm rounded-lg p-4">
						Recipe triggered. The commands are queued and will run when the
						device checks in.
					</div>
				) : isLoadingRecipes ? (
					<p className="text-sm text-gray-500">Loading recipes...</p>
				) : recipes.length === 0 ? (
					<p className="text-sm text-gray-500">
						No recipes yet. Create one on the Recipes page.
					</p>
				) : (
					<div className="space-y-2 max-h-72 overflow-y-auto">
						{recipeError && (
							<div className="bg-red-50 border border-red-200 text-red-800 text-sm rounded-lg p-3">
								{recipeError}
							</div>
						)}
						{recipes.map((r) => (
							<label
								key={r.id}
								className="flex items-start gap-3 px-3 py-2 border border-gray-200 rounded-md cursor-pointer hover:bg-gray-50"
							>
								<input
									type="radio"
									name="recipe"
									className="mt-1"
									checked={selectedRecipeId === r.id}
									onChange={() => setSelectedRecipeId(r.id)}
								/>
								<div className="min-w-0">
									<p className="text-sm font-medium text-gray-900">{r.name}</p>
									{r.description && (
										<p className="text-xs text-gray-500">{r.description}</p>
									)}
									<p className="text-xs text-gray-400 mt-0.5">
										{Array.isArray(r.commands) ? r.commands.length : 0} commands
									</p>
								</div>
							</label>
						))}
					</div>
				)}
			</Modal>
		</div>
	);
};

export default DeviceHeader;
