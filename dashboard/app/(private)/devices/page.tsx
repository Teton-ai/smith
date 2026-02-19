"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
	AlertTriangle,
	Calendar,
	Check,
	CheckCircle,
	ChevronDown,
	Cpu,
	GitBranch,
	Layers,
	Loader2,
	Search,
	Tag,
	Terminal,
	User,
	X,
	XCircle,
} from "lucide-react";
import moment from "moment";
import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "@/app/components/button";
import LabelAutocomplete from "@/app/components/LabelAutocomplete";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import {
	type Device,
	type DistributionRolloutStats,
	type Release,
	useApproveDevice,
	useDeleteDevice,
	useGetDevicesInfinite,
	useGetDistributionRollouts,
	useGetReleases,
	useIssueCommandsToDevices,
	useUpdateDevicesTargetRelease,
} from "../../api-client";

const Tooltip = ({
	children,
	content,
}: {
	children: React.ReactNode;
	content: string;
}) => {
	const [isVisible, setIsVisible] = useState(false);
	const [position, setPosition] = useState<"top" | "right">("top");
	const containerRef = useRef<HTMLDivElement>(null);

	const handleMouseEnter = () => {
		setIsVisible(true);
		if (containerRef.current) {
			const rect = containerRef.current.getBoundingClientRect();

			// If tooltip would be cut off on the left side, position it to the right
			if (rect.left < 150) {
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
					<div className="absolute bottom-full left-1/2 transform -translate-x-1/2 mb-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50">
						{content}
						<div className="absolute top-full left-1/2 transform -translate-x-1/2 w-0 h-0 border-l-4 border-r-4 border-t-4 border-l-transparent border-r-transparent border-t-gray-800"></div>
					</div>
				) : (
					<div className="absolute left-full top-1/2 transform -translate-y-1/2 ml-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50">
						{content}
						<div className="absolute right-full top-1/2 transform -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-r-4 border-t-transparent border-b-transparent border-r-gray-800"></div>
					</div>
				))}
		</div>
	);
};

const DeviceSkeleton = () => (
	<div className="px-4 py-3 animate-pulse">
		<div className="grid grid-cols-8 gap-4 items-center">
			<div className="col-span-2">
				<div className="flex items-center space-x-3">
					<div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0"></div>
					<div className="min-w-0 flex-1">
						<div className="flex items-center space-x-2">
							<div className="w-2 h-2 bg-gray-300 rounded-full flex-shrink-0"></div>
							<div className="h-4 bg-gray-300 rounded w-32"></div>
						</div>
						<div className="h-3 bg-gray-200 rounded w-24 mt-1"></div>
					</div>
				</div>
			</div>

			<div className="col-span-2">
				<div className="flex gap-1">
					<div className="h-5 bg-gray-300 rounded-full w-16"></div>
					<div className="h-5 bg-gray-300 rounded-full w-16"></div>
				</div>
			</div>

			<div className="col-span-2">
				<div className="flex items-center space-x-2">
					<div className="w-4 h-3 bg-gray-300 rounded-sm flex-shrink-0"></div>
					<div className="h-4 bg-gray-300 rounded w-20"></div>
				</div>
			</div>

			<div className="col-span-1">
				<div className="h-4 bg-gray-300 rounded w-20"></div>
			</div>

			<div className="col-span-1">
				<div className="flex items-center space-x-1">
					<div className="w-3 h-3 bg-gray-300 rounded flex-shrink-0"></div>
					<div className="h-3 bg-gray-300 rounded w-12"></div>
				</div>
			</div>
		</div>
	</div>
);

const LoadingSkeleton = () => (
	<div className="divide-y divide-gray-200">
		{Array.from({ length: 6 }, (_, i) => (
			<DeviceSkeleton key={i} />
		))}
	</div>
);

const PAGE_SIZE = 100;

const DevicesPage = () => {
	const router = useRouter();
	const searchParams = useSearchParams();
	const queryClient = useQueryClient();
	const [searchTerm, setSearchTerm] = useState("");
	const [debouncedSearchTerm, setDebouncedSearchTerm] = useState("");
	const [showOutdatedOnly, setShowOutdatedOnly] = useState(false);
	const [showPendingApproval, setShowPendingApproval] = useState(false);
	const [labelFilters, setLabelFilters] = useState<string[]>([]);
	const [onlineStatusFilter, setOnlineStatusFilter] = useState<
		"all" | "online" | "offline"
	>("all");
	const [isSearching, setIsSearching] = useState(false);
	const [releaseFilter, setReleaseFilter] = useState<number | undefined>(
		undefined,
	);
	const [distributionFilter, setDistributionFilter] = useState<
		number | undefined
	>(undefined);
	const [showReleaseDropdown, setShowReleaseDropdown] = useState(false);
	const [releaseSearchQuery, setReleaseSearchQuery] = useState("");
	const releaseDropdownRef = useRef<HTMLDivElement>(null);

	// Bulk deploy state
	const [selectedDeviceIds, setSelectedDeviceIds] = useState<Set<number>>(
		new Set(),
	);
	const [showBulkDeployModal, setShowBulkDeployModal] = useState(false);
	const [selectedReleaseId, setSelectedReleaseId] = useState<
		number | undefined
	>(undefined);
	const [bulkDeployReleaseSearch, setBulkDeployReleaseSearch] = useState("");
	const [mounted, setMounted] = useState(false);

	// Bulk command state
	const [showBulkCommandModal, setShowBulkCommandModal] = useState(false);
	const [freeFormCommand, setFreeFormCommand] = useState("");

	// Approval state
	const [approveModalDevice, setApproveModalDevice] = useState<Device | null>(
		null,
	);
	const [selectedDistribution, setSelectedDistribution] = useState<
		string | null
	>(null);
	const [toast, setToast] = useState<{
		message: string;
		type: "success" | "error";
	} | null>(null);

	const formatRelativeTime = (dateString: string) => {
		return moment(dateString).fromNow();
	};

	useEffect(() => {
		setMounted(true);
	}, []);

	// Toast auto-dismiss
	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	const approveDeviceHook = useApproveDevice();
	const deleteDeviceHook = useDeleteDevice();

	const {
		data: devicesData,
		isLoading: loading,
		fetchNextPage,
		hasNextPage,
		isFetchingNextPage,
		queryKey: devicesQueryKey,
	} = useGetDevicesInfinite(
		{
			labels: labelFilters.length > 0 ? labelFilters : undefined,
			online:
				onlineStatusFilter === "online"
					? true
					: onlineStatusFilter === "offline"
						? false
						: undefined,
			search: debouncedSearchTerm.trim() || undefined,
			outdated: showOutdatedOnly || undefined,
			approved: showPendingApproval ? false : undefined,
			release_id: releaseFilter,
			distribution_id: distributionFilter,
			limit: PAGE_SIZE,
		},
		{
			query: {
				initialPageParam: 0,
				getNextPageParam: (lastPage, allPages) => {
					if (!lastPage || lastPage.length < PAGE_SIZE) return undefined;
					return allPages.length * PAGE_SIZE;
				},
			},
		},
	);

	const filteredDevices = useMemo(
		() =>
			(devicesData?.pages || [])
				.filter((page): page is Device[] => Array.isArray(page))
				.flat()
				.filter(
					(d): d is Device => d != null && typeof d === "object" && "id" in d,
				),
		[devicesData],
	);

	// Fetch all releases
	const { data: allReleases = [] } = useGetReleases();

	// Group stable releases by distribution for the approve modal
	const approvalDistributionMap = useMemo(() => {
		const grouped: Record<
			string,
			{ id: number; latestRelease: Release; count: number }
		> = {};
		const sorted = [...allReleases]
			.filter((r) => !r.draft && !r.yanked)
			.sort(
				(a, b) =>
					new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
			);
		for (const release of sorted) {
			const distName = release.distribution_name || "Unknown";
			if (!grouped[distName]) {
				grouped[distName] = {
					id: release.distribution_id,
					latestRelease: release,
					count: 1,
				};
			} else {
				grouped[distName].count++;
			}
		}
		return grouped;
	}, [allReleases]);

	const selectedApprovalRelease = selectedDistribution
		? approvalDistributionMap[selectedDistribution]?.latestRelease
		: null;

	// Fetch distribution rollouts to filter out empty distributions
	const {
		data: distributionRollouts = new Map<number, DistributionRolloutStats>(),
	} = useGetDistributionRollouts({
		query: {
			select: (data) => {
				return data.reduce((prev, curr) => {
					prev.set(curr.distribution_id, curr);
					return prev;
				}, new Map<number, DistributionRolloutStats>());
			},
		},
	});

	// Group releases by distribution for the dropdown
	const releasesByDistribution = useMemo(() => {
		const grouped: Record<string, { id: number; releases: Release[] }> = {};
		allReleases.forEach((release: Release) => {
			const distName = release.distribution_name || "Unknown";
			if (!grouped[distName]) {
				grouped[distName] = { id: release.distribution_id, releases: [] };
			}
			grouped[distName].releases.push(release);
		});
		// Sort releases within each distribution by version (newest first)
		Object.keys(grouped).forEach((distName) => {
			grouped[distName].releases.sort(
				(a, b) =>
					new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
			);
		});
		// Filter out distributions that only have draft releases (no published releases)
		// AND filter out distributions with 0 devices
		const filteredGrouped: Record<string, { id: number; releases: Release[] }> =
			{};
		Object.entries(grouped).forEach(([distName, data]) => {
			const hasPublishedRelease = data.releases.some((r) => !r.draft);
			const rollout = distributionRollouts.get(data.id);
			const hasDevices = rollout && (rollout.total_devices || 0) > 0;
			if (hasPublishedRelease && hasDevices) {
				filteredGrouped[distName] = data;
			}
		});
		return filteredGrouped;
	}, [allReleases, distributionRollouts]);

	// Get the selected release info for display
	const selectedRelease = useMemo(() => {
		if (releaseFilter == null) return null;
		return allReleases.find((r: Release) => r.id === releaseFilter) || null;
	}, [releaseFilter, allReleases]);

	// Get the selected distribution name for display
	const selectedDistributionName = useMemo(() => {
		if (distributionFilter == null) return null;
		for (const [distName, data] of Object.entries(releasesByDistribution)) {
			if (data.id === distributionFilter) {
				return distName;
			}
		}
		return null;
	}, [distributionFilter, releasesByDistribution]);

	// Sort distributions by latest release date (newest first)
	const sortedDistributions = useMemo(() => {
		return Object.entries(releasesByDistribution)
			.map(([distName, data]) => ({
				name: distName,
				id: data.id,
				releases: data.releases,
				latestReleaseDate: data.releases[0]?.created_at || "",
			}))
			.sort(
				(a, b) =>
					new Date(b.latestReleaseDate).getTime() -
					new Date(a.latestReleaseDate).getTime(),
			);
	}, [releasesByDistribution]);

	// State for expanded distributions in the dropdown
	const [expandedDistributions, setExpandedDistributions] = useState<
		Set<number>
	>(new Set());

	// Bulk deploy: Get selected devices and validate distribution
	const selectedDevices = useMemo(() => {
		return filteredDevices.filter((d) => selectedDeviceIds.has(d.id));
	}, [filteredDevices, selectedDeviceIds]);

	const distributionIds = useMemo(() => {
		return new Set(
			selectedDevices
				.map((d) => d.release?.distribution_id)
				.filter((id): id is number => id != null),
		);
	}, [selectedDevices]);

	const hasMixedDistributions = distributionIds.size > 1;

	const availableReleasesForBulkDeploy = useMemo(() => {
		if (hasMixedDistributions || distributionIds.size === 0) return [];
		const distId = Array.from(distributionIds)[0];
		return allReleases.filter(
			(r: Release) => r.distribution_id === distId && !r.draft && !r.yanked,
		);
	}, [allReleases, distributionIds, hasMixedDistributions]);

	// Bulk deploy mutation
	const { mutate: updateDevicesRelease, isPending: isDeploying } =
		useUpdateDevicesTargetRelease({
			mutation: {
				onSuccess: () => {
					setShowBulkDeployModal(false);
					setSelectedDeviceIds(new Set());
					setSelectedReleaseId(undefined);
				},
				onError: (error) => {
					console.error("Failed to deploy:", error);
				},
			},
		});

	const handleBulkDeploy = () => {
		if (!selectedReleaseId) return;
		updateDevicesRelease({
			data: {
				target_release_id: selectedReleaseId,
				devices: Array.from(selectedDeviceIds),
			},
		});
	};

	// Bulk command mutation
	const { mutate: issueCommands, isPending: isIssuingCommands } =
		useIssueCommandsToDevices({
			mutation: {
				onSuccess: () => {
					setShowBulkCommandModal(false);
					setSelectedDeviceIds(new Set());
					setFreeFormCommand("");
				},
				onError: (error) => {
					console.error("Failed to issue commands:", error);
				},
			},
		});

	const handleBulkCommand = () => {
		if (!freeFormCommand.trim()) return;

		issueCommands({
			data: {
				devices: Array.from(selectedDeviceIds),
				commands: [
					{
						command: { FreeForm: { cmd: freeFormCommand } },
						continue_on_error: false,
					},
				],
			},
		});
	};

	// Debounce search term
	useEffect(() => {
		setIsSearching(true);
		const timer = setTimeout(() => {
			setDebouncedSearchTerm(searchTerm);
			setIsSearching(false);
		}, 300);

		return () => {
			clearTimeout(timer);
			setIsSearching(false);
		};
	}, [searchTerm]);

	// Sync URL parameters with component state
	useEffect(() => {
		const outdated = searchParams.get("outdated");
		const online = searchParams.get("online");
		const labelsParam = searchParams.get("labels");
		const releaseIdParam = searchParams.get("release_id");
		const distributionIdParam = searchParams.get("distribution_id");
		const approvedParam = searchParams.get("approved");

		if (outdated === "true") {
			setShowOutdatedOnly(true);
		}

		if (approvedParam === "false") {
			setShowPendingApproval(true);
		}

		if (online) {
			setOnlineStatusFilter(online as "all" | "online" | "offline");
		}

		if (labelsParam) {
			const parsedLabels = labelsParam.split(",");
			setLabelFilters(parsedLabels);
		}

		if (releaseIdParam) {
			const parsedReleaseId = parseInt(releaseIdParam, 10);
			if (!Number.isNaN(parsedReleaseId)) {
				setReleaseFilter(parsedReleaseId);
			}
		}

		if (distributionIdParam) {
			const parsedDistributionId = parseInt(distributionIdParam, 10);
			if (!Number.isNaN(parsedDistributionId)) {
				setDistributionFilter(parsedDistributionId);
			}
		}
	}, [searchParams]);

	// Update URL when filters change
	const updateURL = (params: Record<string, string | undefined>) => {
		const current = new URLSearchParams(Array.from(searchParams.entries()));

		Object.entries(params).forEach(([key, value]) => {
			if (value == null || value === "") {
				current.delete(key);
			} else {
				current.set(key, value);
			}
		});

		const search = current.toString();
		const query = search ? `?${search}` : "";
		router.replace(`/devices${query}`);
	};

	const getDeviceStatus = (device: Device) => {
		if (!device.last_seen) return "offline";

		const lastSeen = new Date(device.last_seen);
		const now = new Date();
		const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);

		return diffMinutes <= 3 ? "online" : "offline";
	};

	const getDeviceName = (device: Device) => device.serial_number;

	const getOSVersion = (device: Device) => {
		const osRelease = device.system_info?.os_release;
		if (!osRelease) return "Unknown";

		if (osRelease.pretty_name) {
			return osRelease.pretty_name;
		}

		if (osRelease.version_id) {
			return `Ubuntu ${osRelease.version_id}`;
		}

		return "Unknown";
	};

	const getReleaseInfo = (device: Device) => {
		if (device.release) {
			return {
				distribution: device.release.distribution_name,
				version: device.release.version,
			};
		}
		return null;
	};

	const getIpLocationInfo = (device: Device) => {
		return device.ip_address || null;
	};

	const getFlagUrl = (countryCode: string) => {
		return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
	};

	const getStatusTooltip = (device: Device) => {
		const status = getDeviceStatus(device);
		const networkScore = device.network?.network_score;
		const downloadSpeed = device.network?.download_speed_mbps;
		const uploadSpeed = device.network?.upload_speed_mbps;
		const lastSeenText = device.last_seen
			? `${formatTimeAgo(new Date(device.last_seen))} ago`
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
		const lastTested = device.network?.updated_at
			? `${formatTimeAgo(new Date(device.network.updated_at))} ago`
			: "never";

		return `Online - ${qualityText} Network (${networkScore}/5)${speedText}\nLast tested: ${lastTested}\nLast seen: ${lastSeenText}`;
	};

	const hasUpdatePending = (device: Device) => {
		return (
			device.release_id &&
			device.target_release_id &&
			device.release_id !== device.target_release_id
		);
	};

	const handleOutdatedToggle = () => {
		const newValue = !showOutdatedOnly;
		setShowOutdatedOnly(newValue);
		updateURL({ outdated: newValue ? "true" : undefined });
	};

	const handlePendingApprovalToggle = () => {
		const newValue = !showPendingApproval;
		setShowPendingApproval(newValue);
		setSelectedDeviceIds(new Set());
		updateURL({ approved: newValue ? "false" : undefined });
	};

	const handleApproveAndAssign = async () => {
		if (!selectedApprovalRelease) return;

		const deviceIds = Array.from(selectedDeviceIds);
		const count = deviceIds.length;

		setApproveModalDevice(null);

		try {
			for (const deviceId of deviceIds) {
				await approveDeviceHook.mutateAsync({
					deviceId,
					data: { target_release_id: selectedApprovalRelease.id },
				});
			}

			queryClient.invalidateQueries({ queryKey: devicesQueryKey });
			setToast({
				message: `${count} device${count > 1 ? "s" : ""} approved → ${selectedDistribution} ${selectedApprovalRelease.version}`,
				type: "success",
			});
			setSelectedDeviceIds(new Set());
		} catch {
			setToast({
				message: `Failed to approve some devices`,
				type: "error",
			});
		} finally {
			setSelectedDistribution(null);
		}
	};

	const addLabelFilter = (labelInput: string) => {
		const parts = labelInput.split("=");
		if (parts.length === 2) {
			const [key, value] = parts;
			const newFilters = [...labelFilters, `${key.trim()}=${value.trim()}`];
			setLabelFilters(newFilters);

			// Update URL
			const labelsString = newFilters.join(",");
			updateURL({ labels: labelsString });
		}
	};

	const removeLabelFilter = (labelFilter: string) => {
		const newFilters = structuredClone(labelFilters).filter(
			(currentLabelFilter) => currentLabelFilter !== labelFilter,
		);
		setLabelFilters(newFilters);

		// Update URL
		const labelsString =
			newFilters.length > 0 ? newFilters.join(",") : undefined;
		updateURL({ labels: labelsString });
	};

	const handleOnlineStatusChange = (status: "all" | "online" | "offline") => {
		setOnlineStatusFilter(status);
		updateURL({ online: status === "all" ? undefined : status });
	};

	const handleReleaseFilterChange = (releaseId: number | undefined) => {
		setReleaseFilter(releaseId);
		setDistributionFilter(undefined); // Clear distribution filter when selecting a release
		setShowReleaseDropdown(false);
		setReleaseSearchQuery("");
		updateURL({
			release_id: releaseId?.toString(),
			distribution_id: undefined,
		});
	};

	const handleDistributionFilterChange = (
		distributionId: number | undefined,
	) => {
		setDistributionFilter(distributionId);
		setReleaseFilter(undefined); // Clear release filter when selecting a distribution
		setShowReleaseDropdown(false);
		setReleaseSearchQuery("");
		updateURL({
			distribution_id: distributionId?.toString(),
			release_id: undefined,
		});
	};

	// Close dropdown when clicking outside
	useEffect(() => {
		const handleClickOutside = (event: MouseEvent) => {
			if (
				releaseDropdownRef.current &&
				!releaseDropdownRef.current.contains(event.target as Node)
			) {
				setShowReleaseDropdown(false);
				setReleaseSearchQuery("");
			}
		};

		if (showReleaseDropdown) {
			document.addEventListener("mousedown", handleClickOutside);
		}

		return () => {
			document.removeEventListener("mousedown", handleClickOutside);
		};
	}, [showReleaseDropdown]);

	const formatTimeAgo = (date: Date) => {
		const now = new Date();
		const diff = now.getTime() - date.getTime();
		const minutes = Math.floor(diff / 60000);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days}d`;
		if (hours > 0) return `${hours}h`;
		return `${minutes}m`;
	};

	return (
		<div className="space-y-6">
			{/* Toast Notification */}
			{toast && (
				<div
					className={`fixed top-4 right-4 z-50 p-4 rounded-lg shadow-lg border ${
						toast.type === "success"
							? "bg-green-50 text-green-800 border-green-200"
							: "bg-red-50 text-red-800 border-red-200"
					} transition-all duration-300 ease-in-out`}
				>
					<div className="flex items-center space-x-2">
						{toast.type === "success" ? (
							<Check className="w-5 h-5 text-green-600" />
						) : (
							<X className="w-5 h-5 text-red-600" />
						)}
						<span className="text-sm font-medium">{toast.message}</span>
						<button
							onClick={() => setToast(null)}
							className="ml-2 text-gray-400 hover:text-gray-600 cursor-pointer"
						>
							<X className="w-4 h-4" />
						</button>
					</div>
				</div>
			)}

			{/* Search and Filters */}
			<div className="flex flex-col space-y-3">
				<div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
					<div className="flex flex-wrap items-center gap-3">
						<div className="relative">
							<Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
							<input
								type="text"
								placeholder="Search devices..."
								value={searchTerm}
								onChange={(e) => setSearchTerm(e.target.value)}
								className="pl-10 pr-10 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
							/>
							{isSearching && (
								<Loader2 className="absolute right-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4 animate-spin" />
							)}
						</div>

						{/* Online Status Filter */}
						<div className="flex space-x-1">
							<Button
								variant={onlineStatusFilter === "all" ? "primary" : "secondary"}
								onClick={() => handleOnlineStatusChange("all")}
							>
								All
							</Button>
							<Button
								variant={onlineStatusFilter === "online" ? "success" : "secondary"}
								onClick={() => handleOnlineStatusChange("online")}
							>
								Online
							</Button>
							<Button
								variant={onlineStatusFilter === "offline" ? "secondary" : "secondary"}
								className={onlineStatusFilter === "offline" ? "bg-gray-600 hover:bg-gray-700 text-white" : ""}
								onClick={() => handleOnlineStatusChange("offline")}
							>
								Offline
							</Button>
						</div>

						{/* Outdated Filter */}
						<Button
							variant={showOutdatedOnly ? "warning" : "secondary"}
							className={showOutdatedOnly ? "bg-orange-600 hover:bg-orange-700" : ""}
							onClick={handleOutdatedToggle}
						>
							Outdated
						</Button>

						{/* Pending Approval Filter */}
						<Button
							variant={showPendingApproval ? "warning" : "secondary"}
							className={showPendingApproval ? "bg-orange-600 hover:bg-orange-700" : ""}
							onClick={handlePendingApprovalToggle}
						>
							Pending Approval
						</Button>

						{/* Release Filter Dropdown */}
						<div className="relative" ref={releaseDropdownRef}>
							<button
								onClick={() => setShowReleaseDropdown(!showReleaseDropdown)}
								className={`flex items-center space-x-2 px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
									releaseFilter != null || distributionFilter != null
										? "bg-purple-600 text-white"
										: "bg-gray-100 text-gray-700 hover:bg-gray-200"
								}`}
							>
								<GitBranch className="w-4 h-4" />
								<span>
									{selectedRelease
										? `${selectedRelease.distribution_name} ${selectedRelease.version}`
										: selectedDistributionName
											? `${selectedDistributionName} (all)`
											: "Release"}
								</span>
								<ChevronDown
									className={`w-4 h-4 transition-transform ${showReleaseDropdown ? "rotate-180" : ""}`}
								/>
							</button>

							{showReleaseDropdown && (
								<div className="absolute top-full left-0 mt-1 w-72 bg-white border border-gray-200 rounded-md shadow-lg z-50">
									{/* Search input */}
									<div className="p-2 border-b border-gray-200">
										<div className="relative">
											<Search className="absolute left-2.5 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-400" />
											<input
												type="text"
												placeholder="Search releases..."
												value={releaseSearchQuery}
												onChange={(e) => setReleaseSearchQuery(e.target.value)}
												className="w-full pl-8 pr-3 py-1.5 text-sm border border-gray-200 rounded focus:ring-1 focus:ring-purple-500 focus:border-purple-500 text-gray-900 placeholder-gray-400"
												onClick={(e) => e.stopPropagation()}
											/>
										</div>
									</div>

									<div className="max-h-64 overflow-y-auto">
										{(releaseFilter != null || distributionFilter != null) && (
											<button
												onClick={() => {
													setReleaseFilter(undefined);
													setDistributionFilter(undefined);
													setShowReleaseDropdown(false);
													setReleaseSearchQuery("");
													updateURL({
														release_id: undefined,
														distribution_id: undefined,
													});
												}}
												className="w-full px-3 py-2 text-left text-sm text-gray-700 hover:bg-gray-50 border-b border-gray-200 flex items-center space-x-2 cursor-pointer"
											>
												<X className="w-4 h-4 text-gray-400" />
												<span>Clear filter</span>
											</button>
										)}
										{sortedDistributions.length === 0 ? (
											<div className="px-3 py-4 text-sm text-gray-500 text-center">
												No releases available
											</div>
										) : (
											sortedDistributions.map((dist) => {
												const filteredReleases = dist.releases.filter(
													(release) =>
														releaseSearchQuery === "" ||
														release.version
															.toLowerCase()
															.includes(releaseSearchQuery.toLowerCase()) ||
														dist.name
															.toLowerCase()
															.includes(releaseSearchQuery.toLowerCase()),
												);

												const distMatchesSearch =
													releaseSearchQuery === "" ||
													dist.name
														.toLowerCase()
														.includes(releaseSearchQuery.toLowerCase());

												if (filteredReleases.length === 0 && !distMatchesSearch)
													return null;

												const isExpanded =
													expandedDistributions.has(dist.id) ||
													releaseSearchQuery !== "";

												return (
													<div key={dist.name}>
														<div className="flex items-center">
															<button
																onClick={(e) => {
																	e.stopPropagation();
																	setExpandedDistributions((prev) => {
																		const next = new Set(prev);
																		if (next.has(dist.id)) {
																			next.delete(dist.id);
																		} else {
																			next.add(dist.id);
																		}
																		return next;
																	});
																}}
																className="px-2 py-2 text-gray-400 hover:text-gray-600 cursor-pointer"
															>
																<ChevronDown
																	className={`w-4 h-4 transition-transform ${isExpanded ? "rotate-0" : "-rotate-90"}`}
																/>
															</button>
															<button
																onClick={() =>
																	handleDistributionFilterChange(dist.id)
																}
																className={`flex-1 px-2 py-2 text-left text-sm font-medium flex items-center justify-between cursor-pointer transition-colors ${
																	distributionFilter === dist.id
																		? "bg-purple-50 text-purple-700"
																		: "text-gray-700 hover:bg-gray-50"
																}`}
															>
																<span>{dist.name}</span>
																<span className="text-xs text-gray-400">
																	{dist.releases.length} release
																	{dist.releases.length !== 1 ? "s" : ""}
																</span>
															</button>
														</div>
														{isExpanded && (
															<div className="border-l-2 border-gray-100 ml-4">
																{filteredReleases.map((release) => (
																	<button
																		key={release.id}
																		onClick={() =>
																			handleReleaseFilterChange(release.id)
																		}
																		className={`w-full px-3 py-2 text-left text-sm hover:bg-gray-50 flex items-center justify-between cursor-pointer ${
																			releaseFilter === release.id
																				? "bg-purple-50 text-purple-700"
																				: "text-gray-700"
																		}`}
																	>
																		<div className="flex items-center space-x-2">
																			<Tag className="w-3 h-3 text-gray-400" />
																			<span className="font-mono">
																				{release.version}
																			</span>
																		</div>
																		<div className="flex items-center space-x-2">
																			{release.draft && (
																				<span className="px-1.5 py-0.5 text-xs bg-yellow-100 text-yellow-700 rounded">
																					Draft
																				</span>
																			)}
																			{release.yanked && (
																				<span className="px-1.5 py-0.5 text-xs bg-red-100 text-red-700 rounded">
																					Yanked
																				</span>
																			)}
																		</div>
																	</button>
																))}
															</div>
														)}
													</div>
												);
											})
										)}
									</div>
								</div>
							)}
						</div>

						{/* Label Filter Input */}
						<LabelAutocomplete
							onSelect={addLabelFilter}
							existingFilters={labelFilters}
						/>

						{/* Active Label Filters - inline */}
						{labelFilters.length > 0 &&
							labelFilters.map((filter) => (
								<div
									key={filter}
									className="flex items-center space-x-1 px-2 py-1 text-sm bg-gray-100 text-gray-700 rounded border border-gray-200"
								>
									<code className="font-mono text-xs">{filter}</code>
									<button
										onClick={() => removeLabelFilter(filter)}
										className="text-gray-600 hover:text-gray-800 font-bold cursor-pointer"
									>
										×
									</button>
								</div>
							))}
					</div>
				</div>
			</div>

			{/* Device List */}
			<div className="border border-gray-200 rounded-lg overflow-hidden bg-white">
				<div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
					<div className="grid grid-cols-[auto_2fr_2fr_2fr_1fr_1fr] gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide items-center">
						<div className="w-6 flex items-center justify-center">
							<button
								onClick={() => {
									if (
										selectedDeviceIds.size > 0 &&
										selectedDeviceIds.size === filteredDevices.length
									) {
										setSelectedDeviceIds(new Set());
									} else {
										setSelectedDeviceIds(
											new Set(filteredDevices.map((d) => d.id)),
										);
									}
								}}
								className={`w-5 h-5 rounded-full flex items-center justify-center transition-colors cursor-pointer ${
									selectedDeviceIds.size > 0 &&
									selectedDeviceIds.size === filteredDevices.length
										? "bg-blue-600"
										: selectedDeviceIds.size > 0
											? "bg-blue-400"
											: "border-2 border-gray-300 hover:border-gray-400"
								}`}
							>
								{selectedDeviceIds.size > 0 && (
									<svg
										className="w-3 h-3 text-white"
										fill="none"
										viewBox="0 0 24 24"
										stroke="currentColor"
									>
										<path
											strokeLinecap="round"
											strokeLinejoin="round"
											strokeWidth={2}
											d={
												selectedDeviceIds.size === filteredDevices.length
													? "M5 13l4 4L19 7"
													: "M20 12H4"
											}
										/>
									</svg>
								)}
							</button>
						</div>
						<div>Device</div>
						<div>Labels</div>
						<div>Location</div>
						<div>OS</div>
						<div>Release</div>
					</div>
				</div>

				{loading ? (
					<LoadingSkeleton />
				) : (
					<div className="divide-y divide-gray-200">
						{filteredDevices.map((device) => (
							<div
								key={device.id}
								className="block px-4 py-3 hover:bg-gray-50 cursor-pointer transition-colors"
								onClick={() => router.push(`/devices/${device.serial_number}`)}
							>
								<div className="grid grid-cols-[auto_2fr_2fr_2fr_1fr_1fr] gap-4 items-center">
									<div className="w-6 flex items-center justify-center">
										<button
											onClick={(e) => {
												e.stopPropagation();
												const newSet = new Set(selectedDeviceIds);
												if (selectedDeviceIds.has(device.id)) {
													newSet.delete(device.id);
												} else {
													newSet.add(device.id);
												}
												setSelectedDeviceIds(newSet);
											}}
											className={`w-5 h-5 rounded-full flex items-center justify-center transition-colors cursor-pointer ${
												selectedDeviceIds.has(device.id)
													? "bg-blue-600"
													: "border-2 border-gray-300 hover:border-gray-400"
											}`}
										>
											{selectedDeviceIds.has(device.id) && (
												<svg
													className="w-3 h-3 text-white"
													fill="none"
													viewBox="0 0 24 24"
													stroke="currentColor"
												>
													<path
														strokeLinecap="round"
														strokeLinejoin="round"
														strokeWidth={2}
														d="M5 13l4 4L19 7"
													/>
												</svg>
											)}
										</button>
									</div>
									<div>
										<div className="flex items-center space-x-3">
											<Cpu className="w-4 h-4 text-gray-400 flex-shrink-0" />
											<div className="min-w-0 flex-1">
												<div className="flex items-center space-x-2">
													<Tooltip content={getStatusTooltip(device)}>
														<div className="flex-shrink-0 cursor-help">
															<NetworkQualityIndicator
																isOnline={getDeviceStatus(device) === "online"}
																networkScore={device.network?.network_score}
															/>
														</div>
													</Tooltip>
													<div className="flex items-center space-x-2 min-w-0 flex-1">
														<div className="text-sm font-medium text-gray-900 truncate">
															{getDeviceName(device)}
														</div>
														{hasUpdatePending(device) && (
															<Tooltip
																content={`Update pending: Release ${device.release_id} → ${device.target_release_id}`}
															>
																<span className="px-1.5 py-0.5 text-xs font-medium bg-orange-100 text-orange-800 rounded-full cursor-help flex-shrink-0">
																	Outdated
																</span>
															</Tooltip>
														)}
													</div>
												</div>
											</div>
										</div>
									</div>

									<div>
										{device.labels && Object.keys(device.labels).length > 0 ? (
											<div className="flex flex-wrap gap-1">
												{Object.entries(device.labels).map(([key, value]) => {
													const filter = `${key}=${value}`;
													const isFiltered = labelFilters.includes(filter);
													return (
														<code
															key={key}
															onClick={(e) => {
																e.stopPropagation();
																if (isFiltered) {
																	removeLabelFilter(filter);
																} else {
																	const newFilters = [...labelFilters, filter];
																	setLabelFilters(newFilters);
																	const labelsString = newFilters.join(",");
																	updateURL({ labels: labelsString });
																}
															}}
															className={`px-1.5 py-0.5 text-xs font-mono rounded border cursor-pointer transition-colors ${
																isFiltered
																	? "bg-blue-100 text-blue-800 border-blue-300"
																	: "bg-gray-100 text-gray-700 border-gray-200 hover:bg-gray-200"
															}`}
														>
															{key}={value}
														</code>
													);
												})}
											</div>
										) : (
											<span className="text-xs text-gray-400">-</span>
										)}
									</div>

									<div>
										{(() => {
											const ipInfo = getIpLocationInfo(device);
											if (!ipInfo) {
												return (
													<div className="text-sm text-gray-400">
														No location data
													</div>
												);
											}

											const locationParts = [];
											if (ipInfo.name) locationParts.push(ipInfo.name);
											if (ipInfo.city) locationParts.push(ipInfo.city);
											if (ipInfo.country) locationParts.push(ipInfo.country);

											return (
												<div className="flex items-center space-x-2">
													{ipInfo.country_code && (
														<img
															src={getFlagUrl(ipInfo.country_code)}
															alt={ipInfo.country || "Country flag"}
															className="w-4 h-3 flex-shrink-0 rounded-sm"
															onError={(e) => {
																(e.target as HTMLImageElement).style.display =
																	"none";
															}}
														/>
													)}
													<div className="text-sm text-gray-600 truncate">
														{locationParts.join(", ") || "Unknown location"}
													</div>
												</div>
											);
										})()}
									</div>

									<div className="text-sm text-gray-600">
										{getOSVersion(device)}
									</div>

									<div>
										{getReleaseInfo(device) ? (
											<div className="flex flex-col space-y-1">
												<div className="flex items-center space-x-1">
													<GitBranch className="w-3 h-3 text-gray-400" />
													<span className="text-xs font-mono text-gray-600">
														{getReleaseInfo(device)!.distribution}
													</span>
												</div>
												<div className="flex items-center space-x-1">
													<Tag className="w-3 h-3 text-gray-400" />
													<span className="text-xs font-mono text-gray-600">
														{getReleaseInfo(device)!.version}
													</span>
												</div>
											</div>
										) : (
											<div className="flex items-center space-x-1">
												<GitBranch className="w-3 h-3 text-gray-400" />
												<span className="text-xs text-gray-500">
													No Release
												</span>
											</div>
										)}
									</div>
								</div>
							</div>
						))}
						{/* Load More Button */}
						{hasNextPage && (
							<div className="border-t border-gray-200">
								<button
									onClick={() => fetchNextPage()}
									disabled={isFetchingNextPage}
									className="w-full py-2 px-4 text-sm font-medium text-blue-600 bg-blue-50 rounded-md hover:bg-blue-100 disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer transition-colors"
								>
									{isFetchingNextPage ? (
										<span className="flex items-center justify-center gap-2">
											<Loader2 className="w-4 h-4 animate-spin" />
											Loading more...
										</span>
									) : (
										`Load more devices`
									)}
								</button>
							</div>
						)}
					</div>
				)}
			</div>

			{/* Bulk Action Bar */}
			{selectedDeviceIds.size > 0 && (
				<div className="fixed bottom-0 left-0 right-0 bg-white border-t shadow-lg p-4 flex items-center justify-between z-40">
					<span className="text-gray-700">
						{selectedDeviceIds.size} device
						{selectedDeviceIds.size > 1 ? "s" : ""} selected
					</span>
					<div className="flex gap-2">
						<Button
							variant="secondary"
							onClick={() => setSelectedDeviceIds(new Set())}
						>
							Clear Selection
						</Button>
						{showPendingApproval ? (
							<>
								<Button
									variant="danger"
									icon={<XCircle className="w-4 h-4" />}
									onClick={() => {
										const deviceIds = Array.from(selectedDeviceIds);
										const count = deviceIds.length;
										if (
											!confirm(
												`Are you sure you want to reject ${count} device${count > 1 ? "s" : ""}? This will archive them.`,
											)
										) {
											return;
										}
										Promise.all(
											deviceIds.map((id) =>
												deleteDeviceHook.mutateAsync({ deviceId: id }),
											),
										)
											.then(() => {
												queryClient.invalidateQueries({
													queryKey: devicesQueryKey,
												});
												setToast({
													message: `${count} device${count > 1 ? "s" : ""} rejected and archived`,
													type: "success",
												});
												setSelectedDeviceIds(new Set());
											})
											.catch(() => {
												setToast({
													message: "Failed to reject some devices",
													type: "error",
												});
											});
									}}
								>
									Reject
								</Button>
								<Button
									variant="success"
									icon={<CheckCircle className="w-4 h-4" />}
									onClick={() => {
										setApproveModalDevice(
											filteredDevices.find((d) =>
												selectedDeviceIds.has(d.id),
											) || null,
										);
										setSelectedDistribution(null);
									}}
								>
									Approve & Assign
								</Button>
							</>
						) : (
							<>
								<Button
									variant="purple"
									icon={<Terminal className="w-4 h-4" />}
									onClick={() => setShowBulkCommandModal(true)}
								>
									Run Command
								</Button>
								<Button
									variant="warning"
									onClick={() => setShowBulkDeployModal(true)}
								>
									Deploy to Selected
								</Button>
							</>
						)}
					</div>
				</div>
			)}

			{/* Bulk Deploy Modal */}
			{mounted &&
				showBulkDeployModal &&
				createPortal(
					<div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
						<div className="bg-white rounded-lg shadow-xl p-6 w-[520px] animate-in zoom-in-95 duration-200">
							<div className="flex justify-between items-center mb-4">
								<h2 className="text-xl font-semibold text-gray-900">
									Deploy to Selected Devices
								</h2>
								<button
									onClick={() => {
										setShowBulkDeployModal(false);
										setSelectedReleaseId(undefined);
										setBulkDeployReleaseSearch("");
									}}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
								>
									<X className="w-5 h-5" />
								</button>
							</div>

							{/* Warning Banner */}
							<div className="bg-amber-50 border border-amber-200 rounded-lg p-4 mb-4">
								<div className="flex gap-3">
									<AlertTriangle className="w-5 h-5 text-amber-600 flex-shrink-0 mt-0.5" />
									<div>
										<p className="text-amber-800 font-medium">
											Direct Deployment - Bypasses Canary
										</p>
										<p className="text-amber-700 text-sm mt-1">
											This will deploy directly to {selectedDeviceIds.size}{" "}
											device
											{selectedDeviceIds.size > 1 ? "s" : ""} without the
											standard canary rollout process. Use with caution.
										</p>
									</div>
								</div>
							</div>

							{/* Distribution Mismatch Error */}
							{hasMixedDistributions && (
								<div className="bg-red-50 border border-red-200 rounded-lg p-4 mb-4">
									<p className="text-red-800 text-sm">
										Selected devices belong to different distributions. Please
										select devices from a single distribution.
									</p>
								</div>
							)}

							{/* No Distribution Warning */}
							{!hasMixedDistributions && distributionIds.size === 0 && (
								<div className="bg-yellow-50 border border-yellow-200 rounded-lg p-4 mb-4">
									<p className="text-yellow-800 text-sm">
										Selected devices have no release assigned. Please select
										devices that have a release.
									</p>
								</div>
							)}

							{/* Release Selector */}
							<div className="mb-6">
								<label className="block text-sm font-medium text-gray-700 mb-2">
									Target Release
								</label>
								{hasMixedDistributions || distributionIds.size === 0 ? (
									<div className="text-center py-4 text-gray-500 text-sm border border-gray-200 rounded-md">
										{hasMixedDistributions
											? "Cannot select release for mixed distributions"
											: "No releases available"}
									</div>
								) : availableReleasesForBulkDeploy.length === 0 ? (
									<div className="text-center py-4 text-gray-500 text-sm border border-gray-200 rounded-md">
										No releases available for this distribution
									</div>
								) : (
									<div className="space-y-3">
										{/* Search Input */}
										<div className="relative">
											<Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
											<input
												type="text"
												placeholder="Search releases..."
												value={bulkDeployReleaseSearch}
												onChange={(e) =>
													setBulkDeployReleaseSearch(e.target.value)
												}
												className="w-full pl-10 pr-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-amber-500 focus:border-amber-500 placeholder:text-gray-400"
											/>
										</div>

										{/* Release List */}
										<div className="border border-gray-200 rounded-md max-h-[280px] overflow-y-auto">
											{(() => {
												const filteredReleases = availableReleasesForBulkDeploy
													.filter(
														(release: Release) =>
															bulkDeployReleaseSearch === "" ||
															release.version
																.toLowerCase()
																.includes(
																	bulkDeployReleaseSearch.toLowerCase(),
																) ||
															release.distribution_name
																?.toLowerCase()
																.includes(
																	bulkDeployReleaseSearch.toLowerCase(),
																),
													)
													.sort(
														(a: Release, b: Release) =>
															new Date(b.created_at).getTime() -
															new Date(a.created_at).getTime(),
													);

												if (filteredReleases.length === 0) {
													return (
														<div className="text-center py-4 text-gray-500 text-sm">
															No releases match your search
														</div>
													);
												}

												return filteredReleases.map((release: Release) => (
													<button
														key={release.id}
														onClick={() => setSelectedReleaseId(release.id)}
														className={`w-full text-left p-3 border-b border-gray-200 last:border-b-0 hover:bg-gray-50 transition-colors cursor-pointer ${
															selectedReleaseId === release.id
																? "bg-amber-50 hover:bg-amber-50"
																: ""
														}`}
													>
														<div className="flex items-start justify-between">
															<div className="flex-1 min-w-0">
																<div className="flex items-center space-x-2">
																	<div className="p-1.5 bg-gray-100 text-gray-600 rounded">
																		<Tag className="w-3 h-3" />
																	</div>
																	<span className="font-medium text-gray-900">
																		{release.version}
																	</span>
																	{release.draft && (
																		<span className="px-2 py-0.5 text-xs font-medium bg-yellow-100 text-yellow-800 rounded-full">
																			Draft
																		</span>
																	)}
																	{release.yanked && (
																		<span className="px-2 py-0.5 text-xs font-medium bg-red-100 text-red-800 rounded-full">
																			Yanked
																		</span>
																	)}
																</div>
																<div className="flex items-center space-x-3 mt-1.5 text-xs text-gray-500">
																	<div className="flex items-center space-x-1">
																		<Calendar className="w-3 h-3" />
																		<span>
																			{formatRelativeTime(release.created_at)}
																		</span>
																	</div>
																	<div className="flex items-center space-x-1">
																		<User className="w-3 h-3" />
																		<span>
																			{release.user_email ||
																				(release.user_id
																					? `User #${release.user_id}`
																					: "Unknown")}
																		</span>
																	</div>
																</div>
															</div>
															{selectedReleaseId === release.id && (
																<div className="flex-shrink-0 ml-2">
																	<div className="w-5 h-5 bg-amber-600 rounded-full flex items-center justify-center">
																		<svg
																			className="w-3 h-3 text-white"
																			fill="none"
																			viewBox="0 0 24 24"
																			stroke="currentColor"
																		>
																			<path
																				strokeLinecap="round"
																				strokeLinejoin="round"
																				strokeWidth={2}
																				d="M5 13l4 4L19 7"
																			/>
																		</svg>
																	</div>
																</div>
															)}
														</div>
													</button>
												));
											})()}
										</div>
									</div>
								)}
							</div>

							{/* Action Buttons */}
							<div className="flex justify-end gap-3">
								<button
									onClick={() => {
										setShowBulkDeployModal(false);
										setSelectedReleaseId(undefined);
										setBulkDeployReleaseSearch("");
									}}
									className="px-4 py-2 bg-gray-100 text-gray-700 rounded-md hover:bg-gray-200 cursor-pointer"
								>
									Cancel
								</button>
								<button
									onClick={handleBulkDeploy}
									disabled={
										!selectedReleaseId ||
										hasMixedDistributions ||
										distributionIds.size === 0 ||
										isDeploying
									}
									className="px-4 py-2 bg-amber-600 text-white rounded-md hover:bg-amber-700 disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer"
								>
									{isDeploying ? (
										<span className="flex items-center gap-2">
											<Loader2 className="w-4 h-4 animate-spin" />
											Deploying...
										</span>
									) : (
										"Deploy"
									)}
								</button>
							</div>
						</div>
					</div>,
					document.body,
				)}

			{/* Bulk Command Modal */}
			{mounted &&
				showBulkCommandModal &&
				createPortal(
					<div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
						<div className="bg-white rounded-lg shadow-xl p-6 w-[520px] animate-in zoom-in-95 duration-200">
							<div className="flex justify-between items-center mb-4">
								<h2 className="text-xl font-semibold text-gray-900">
									Run Command on Selected Devices
								</h2>
								<button
									onClick={() => {
										setShowBulkCommandModal(false);
										setFreeFormCommand("");
									}}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
								>
									<X className="w-5 h-5" />
								</button>
							</div>

							{/* Info Banner */}
							<div className="bg-purple-50 border border-purple-200 rounded-lg p-4 mb-4">
								<div className="flex gap-3">
									<Terminal className="w-5 h-5 text-purple-600 flex-shrink-0 mt-0.5" />
									<div>
										<p className="text-purple-800 font-medium">
											Execute Command on {selectedDeviceIds.size} Device
											{selectedDeviceIds.size > 1 ? "s" : ""}
										</p>
										<p className="text-purple-700 text-sm mt-1">
											The command will be queued and executed on all selected
											devices when they check in.
										</p>
									</div>
								</div>
							</div>

							{/* Command Input */}
							<div className="mb-6">
								<label className="block text-sm font-medium text-gray-700 mb-2">
									Command
								</label>
								<input
									type="text"
									value={freeFormCommand}
									onChange={(e) => setFreeFormCommand(e.target.value)}
									placeholder="e.g., ls -la /var/log"
									className="w-full px-3 py-2 border border-gray-300 rounded-md focus:ring-purple-500 focus:border-purple-500 text-gray-900 placeholder-gray-400"
								/>
								<p className="mt-1 text-xs text-gray-500">
									Enter a shell command to execute on the selected devices
								</p>
							</div>

							{/* Action Buttons */}
							<div className="flex justify-end gap-3">
								<button
									onClick={() => {
										setShowBulkCommandModal(false);
										setFreeFormCommand("");
									}}
									className="px-4 py-2 bg-gray-100 text-gray-700 rounded-md hover:bg-gray-200 cursor-pointer"
								>
									Cancel
								</button>
								<button
									onClick={handleBulkCommand}
									disabled={!freeFormCommand.trim() || isIssuingCommands}
									className="px-4 py-2 bg-purple-600 text-white rounded-md hover:bg-purple-700 disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer"
								>
									{isIssuingCommands ? (
										<span className="flex items-center gap-2">
											<Loader2 className="w-4 h-4 animate-spin" />
											Sending...
										</span>
									) : (
										"Run Command"
									)}
								</button>
							</div>
						</div>
					</div>,
					document.body,
				)}

			{/* Approve & Assign Distribution Modal */}
			{mounted &&
				approveModalDevice &&
				createPortal(
					<div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
						<div className="bg-white rounded-lg shadow-xl p-6 w-[520px] animate-in zoom-in-95 duration-200">
							<div className="flex justify-between items-center mb-4">
								<div>
									<h2 className="text-xl font-semibold text-gray-900">
										Approve & Assign Distribution
									</h2>
									<p className="text-sm text-gray-500 mt-0.5">
										{selectedDeviceIds.size} device
										{selectedDeviceIds.size > 1 ? "s" : ""}
									</p>
								</div>
								<button
									onClick={() => setApproveModalDevice(null)}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
								>
									<X className="w-5 h-5" />
								</button>
							</div>

							<label className="block text-sm font-medium text-gray-700 mb-3">
								Select a distribution
							</label>

							{Object.keys(approvalDistributionMap).length === 0 ? (
								<div className="text-center py-6 text-gray-500 text-sm border border-gray-200 rounded-md mb-6">
									No distributions with stable releases available
								</div>
							) : (
								<div className="space-y-2 max-h-64 overflow-y-auto mb-6">
									{Object.entries(approvalDistributionMap).map(
										([distName, { latestRelease, count }]) => (
											<button
												key={distName}
												onClick={() => setSelectedDistribution(distName)}
												className={`w-full text-left p-4 rounded-lg border transition-colors cursor-pointer ${
													selectedDistribution === distName
														? "border-indigo-500 bg-indigo-50"
														: "border-gray-200 hover:border-gray-300 hover:bg-gray-50"
												}`}
											>
												<div className="flex items-center justify-between">
													<div className="flex items-center space-x-3">
														<Layers
															className={`w-5 h-5 ${
																selectedDistribution === distName
																	? "text-indigo-600"
																	: "text-gray-400"
															}`}
														/>
														<div>
															<p className="font-medium text-gray-900">
																{distName}
															</p>
															<p className="text-xs text-gray-500 mt-0.5">
																Latest: {latestRelease.version} ·{" "}
																{formatRelativeTime(latestRelease.created_at)} ·{" "}
																{count} release{count !== 1 ? "s" : ""}
															</p>
														</div>
													</div>
													{selectedDistribution === distName && (
														<div className="w-5 h-5 bg-indigo-600 rounded-full flex items-center justify-center flex-shrink-0">
															<svg
																className="w-3 h-3 text-white"
																fill="none"
																viewBox="0 0 24 24"
																stroke="currentColor"
															>
																<path
																	strokeLinecap="round"
																	strokeLinejoin="round"
																	strokeWidth={2}
																	d="M5 13l4 4L19 7"
																/>
															</svg>
														</div>
													)}
												</div>
											</button>
										),
									)}
								</div>
							)}

							<div className="flex justify-end space-x-3">
								<button
									onClick={() => setApproveModalDevice(null)}
									className="px-4 py-2 text-sm font-medium text-gray-700 bg-white border border-gray-300 rounded-md hover:bg-gray-50 transition-colors cursor-pointer"
								>
									Cancel
								</button>
								<button
									onClick={handleApproveAndAssign}
									disabled={!selectedApprovalRelease}
									className="flex items-center space-x-2 px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded-md transition-colors cursor-pointer"
								>
									<CheckCircle className="w-4 h-4" />
									<span>
										{selectedApprovalRelease
											? `Approve → ${selectedApprovalRelease.version}`
											: "Approve & Assign"}
									</span>
								</button>
							</div>
						</div>
					</div>,
					document.body,
				)}
		</div>
	);
};

export default DevicesPage;
