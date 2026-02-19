"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
	AlertTriangle,
	ArrowLeft,
	ArrowUp,
	Box,
	Calendar,
	CheckCircle,
	ChevronRight,
	ChevronsUpDown,
	Clock,
	Cog,
	Cpu,
	Eye,
	Loader2,
	Package as PackageIcon,
	Plus,
	Rocket,
	Search,
	Tag,
	Trash2,
	X,
	XCircle,
	Zap,
} from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import {
	type DeploymentRequest,
	type Device,
	type Package,
	useAddPackageToRelease,
	useApiGetReleaseDeployment,
	useApiReleaseDeployment,
	useDeletePackageForRelease,
	useGetDevices,
	useGetDistributionById,
	useGetDistributionReleasePackages,
	useGetPackages,
	useGetRelease,
	useGetReleaseServices,
	usePatchRelease,
} from "@/app/api-client";
import LabelAutocomplete from "@/app/components/LabelAutocomplete";
import { Button, IconButton } from "@/app/components/button";
import { Modal } from "@/app/components/modal";

const ReleaseDetailPage = () => {
	const router = useRouter();
	const params = useParams();
	const releaseId = parseInt(params.id as string);
	const queryClient = useQueryClient();
	const [showAddPackageModal, setShowAddPackageModal] = useState(false);
	const [showReplacePackageModal, setShowReplacePackageModal] = useState(false);
	const [packageToReplace, setPackageToReplace] = useState<Package | null>(
		null,
	);
	const [selectedAvailablePackage, setSelectedAvailablePackage] = useState<
		number | null
	>(null);
	const [packageSearchQuery, setPackageSearchQuery] = useState("");
	const [showDeployModal, setShowDeployModal] = useState(false);
	const [deploying, setDeploying] = useState(false);
	const [canaryMode, setCanaryMode] = useState<
		"automatic" | "labels" | "devices"
	>("automatic");
	const [canaryLabels, setCanaryLabels] = useState<string[]>([]);
	const [canaryDeviceIds, setCanaryDeviceIds] = useState<Set<number>>(
		new Set(),
	);
	const [deviceSearchQuery, setDeviceSearchQuery] = useState("");
	const [deployStep, setDeployStep] = useState<1 | 2>(1);
	const [showYankModal, setShowYankModal] = useState(false);
	const [yanking, setYanking] = useState(false);
	const [yankReason, setYankReason] = useState("");
	const [toast, setToast] = useState<{
		message: string;
		type: "success" | "error";
	} | null>(null);
	const [upgradingPackages, setUpgradingPackages] = useState<Set<number>>(
		new Set(),
	);

	const {
		data: release,
		isLoading: loading,
		queryKey: releaseQueryKey,
	} = useGetRelease(releaseId);

	const { data: distribution } = useGetDistributionById(
		release?.distribution_id as any,
		{
			query: {
				enabled: !!release?.distribution_id,
			},
		},
	);

	const {
		data: packages = [],
		isLoading: packagesLoading,
		queryKey: packagesQueryKey,
	} = useGetDistributionReleasePackages(releaseId);

	const { data: availablePackages = [], isLoading: availablePackagesLoading } =
		useGetPackages();

	const { data: services = [], isLoading: servicesLoading } =
		useGetReleaseServices(releaseId, {
			query: {
				enabled: !!releaseId,
				refetchInterval: 5000,
			},
		});

	const { data: existingDeployment } = useApiGetReleaseDeployment(releaseId, {
		query: {
			enabled: !!releaseId && !!release && !release.draft && !release.yanked,
			retry: false,
		},
	});

	// Eligible devices: online + up-to-date in distribution (used for devices mode)
	const { data: eligibleDevices = [], isLoading: eligibleDevicesLoading } =
		useGetDevices(
			{
				distribution_id: release?.distribution_id,
				online: true,
				outdated: false,
			},
			{
				query: {
					enabled:
						showDeployModal &&
						canaryMode === "devices" &&
						!!release?.distribution_id,
				},
			},
		);

	// Devices matching selected labels (used for labels mode preview)
	const { data: labelMatchDevices = [], isLoading: labelDevicesLoading } =
		useGetDevices(
			{
				distribution_id: release?.distribution_id,
				outdated: false,
				labels: canaryLabels,
			},
			{
				query: {
					enabled:
						showDeployModal &&
						canaryMode === "labels" &&
						canaryLabels.length > 0 &&
						!!release?.distribution_id,
				},
			},
		);

	const filteredEligibleDevices = useMemo(() => {
		if (!deviceSearchQuery) return eligibleDevices;
		const q = deviceSearchQuery.toLowerCase();
		return eligibleDevices.filter((d: Device) =>
			d.serial_number.toLowerCase().includes(q),
		);
	}, [eligibleDevices, deviceSearchQuery]);

	const [mounted, setMounted] = useState(false);
	useEffect(() => {
		setMounted(true);
	}, []);

	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	const formatRelativeTime = (dateString: string) => {
		return moment(dateString).fromNow();
	};

	const getArchColor = (architecture: string) => {
		switch (architecture.toLowerCase()) {
			case "x86_64":
			case "amd64":
				return "bg-blue-100 text-blue-700";
			case "arm64":
			case "aarch64":
				return "bg-green-100 text-green-700";
			case "armv7":
			case "arm":
				return "bg-purple-100 text-purple-700";
			default:
				return "bg-gray-100 text-gray-700";
		}
	};

	const updateReleaseHook = usePatchRelease({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: releaseQueryKey });
			},
		},
	});
	const handlePublishRelease = async () => {
		updateReleaseHook.mutate({ releaseId, data: { draft: false } });
	};

	const addPackageToReleaseHook = useAddPackageToRelease({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: packagesQueryKey });
				setSelectedAvailablePackage(null);
				setShowAddPackageModal(false);
			},
		},
	});
	const handleAddPackage = async () => {
		if (!selectedAvailablePackage) return;
		addPackageToReleaseHook.mutate({
			releaseId,
			data: { id: selectedAvailablePackage },
		});
	};

	const deletePackageForReleaseHook = useDeletePackageForRelease({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({
					queryKey: packagesQueryKey,
				});
			},
		},
	});
	const handleDeletePackage = async (packageId: number) => {
		if (!confirm("Are you sure you want to delete this package?")) return;
		deletePackageForReleaseHook.mutate({ releaseId, packageId });
	};

	const openAddModal = () => {
		setSelectedAvailablePackage(null);
		setPackageSearchQuery("");
		setShowAddPackageModal(true);
	};

	const openReplaceModal = (pkg: Package) => {
		setPackageToReplace(pkg);
		setSelectedAvailablePackage(null);
		setPackageSearchQuery("");
		setShowReplacePackageModal(true);
	};

	const handleReplacePackage = async () => {
		if (!selectedAvailablePackage || !packageToReplace) return;

		try {
			// Delete old package
			await deletePackageForReleaseHook.mutateAsync({
				releaseId,
				packageId: packageToReplace.id,
			});

			await addPackageToReleaseHook.mutateAsync({
				releaseId,
				data: { id: selectedAvailablePackage },
			});

			queryClient.invalidateQueries({
				queryKey: packagesQueryKey,
			});
			setSelectedAvailablePackage(null);
			setPackageToReplace(null);
			setShowReplacePackageModal(false);
		} catch (error: any) {
			console.error("Failed to replace package:", error);
			alert(`Failed to replace package: ${error?.message || "Unknown error"}`);
		}
	};

	const compareVersions = (v1: string, v2: string): number => {
		const parts1 = v1.replace(/^v/, "").split(".").map(Number);
		const parts2 = v2.replace(/^v/, "").split(".").map(Number);

		for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
			const part1 = parts1[i] || 0;
			const part2 = parts2[i] || 0;
			if (part1 > part2) return 1;
			if (part1 < part2) return -1;
		}
		return 0;
	};

	const getLatestVersionForPackage = (pkg: Package) => {
		if (availablePackages.length === 0) return null;

		// Find the current package's created_at from availablePackages
		const currentPkg = availablePackages.find((p) => p.id === pkg.id);
		const currentCreatedAt = currentPkg
			? new Date(currentPkg.created_at).getTime()
			: 0;

		const newerPackages = availablePackages.filter(
			(availPkg) =>
				availPkg.name === pkg.name &&
				availPkg.id !== pkg.id &&
				(!distribution ||
					availPkg.architecture === distribution.architecture) &&
				new Date(availPkg.created_at).getTime() > currentCreatedAt,
		);

		if (newerPackages.length === 0) return null;

		// Sort by created_at to get the most recently created
		const sorted = [...newerPackages].sort(
			(a, b) =>
				new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
		);

		return sorted[0];
	};

	const handleUpgradePackage = async (pkg: Package) => {
		const latestVersion = getLatestVersionForPackage(pkg);
		if (!latestVersion) return;

		setUpgradingPackages((prev) => new Set(prev).add(pkg.id));

		try {
			// Delete old package
			await deletePackageForReleaseHook.mutateAsync({
				releaseId,
				packageId: pkg.id,
			});

			await addPackageToReleaseHook.mutateAsync({
				releaseId,
				data: { id: latestVersion.id },
			});

			queryClient.invalidateQueries({
				queryKey: packagesQueryKey,
			});

			setToast({
				message: `Upgraded ${pkg.name} to v${latestVersion.version}`,
				type: "success",
			});
		} catch (error: any) {
			console.error("Failed to upgrade package:", error);
			setToast({
				message: `Failed to upgrade ${pkg.name}: ${error?.message || "Unknown error"}`,
				type: "error",
			});
		} finally {
			setUpgradingPackages((prev) => {
				const newSet = new Set(prev);
				newSet.delete(pkg.id);
				return newSet;
			});
		}
	};

	const handleOpenDeployModal = () => {
		setCanaryMode("automatic");
		setCanaryLabels([]);
		setCanaryDeviceIds(new Set());
		setDeviceSearchQuery("");
		setDeployStep(1);
		setShowDeployModal(true);
	};

	const releaseDeploymentHook = useApiReleaseDeployment();

	const handleDeployRelease = async () => {
		if (!release || deploying) return;

		let data: DeploymentRequest = {};
		if (canaryMode === "labels" && canaryLabels.length > 0) {
			data = { canary_device_labels: canaryLabels };
		} else if (canaryMode === "devices" && canaryDeviceIds.size > 0) {
			data = { canary_device_ids: Array.from(canaryDeviceIds) };
		}

		setDeploying(true);
		try {
			await releaseDeploymentHook.mutateAsync({ releaseId, data });
			setShowDeployModal(false);
			router.push(`/releases/${releaseId}/deployment`);
		} catch (error: any) {
			console.error("Failed to deploy release:", error);
			alert(`Failed to deploy release: ${error?.message || "Unknown error"}`);
		} finally {
			setDeploying(false);
		}
	};

	const handleYankRelease = async () => {
		if (!release || yanking) return;

		if (!yankReason.trim()) {
			setToast({
				message: "Please provide a reason for yanking this release",
				type: "error",
			});
			return;
		}

		setYanking(true);
		try {
			await updateReleaseHook.mutateAsync({
				releaseId,
				data: { yanked: true },
			});

			queryClient.invalidateQueries({ queryKey: releaseQueryKey });
			setShowYankModal(false);
			setYankReason("");
			setToast({
				message: "Release has been yanked",
				type: "success",
			});
		} catch (error: any) {
			console.error("Failed to yank release:", error);
			setToast({
				message: `Failed to yank release: ${error?.message || "Unknown error"}`,
				type: "error",
			});
		} finally {
			setYanking(false);
		}
	};

	const getPackageNameForService = (packageId?: number): string | null => {
		if (packageId == null) return null;
		const pkg = packages.find((p) => p.id === packageId);
		return pkg ? `${pkg.name} v${pkg.version}` : null;
	};

	if (loading || !release) {
		return (
			<div className="flex items-center justify-center h-32">
				<div className="text-gray-500 text-sm">Loading...</div>
			</div>
		);
	}

	return (
		<>
			{/* Toast Notification */}
			{mounted &&
				toast &&
				createPortal(
					<div className="fixed top-4 right-4 z-50 animate-in slide-in-from-top-5 duration-300">
						<div
							className={`flex items-center space-x-3 px-4 py-3 rounded-lg shadow-lg ${
								toast.type === "success" ? "bg-green-600" : "bg-red-600"
							}`}
						>
							{toast.type === "success" ? (
								<CheckCircle className="w-5 h-5 text-white" />
							) : (
								<XCircle className="w-5 h-5 text-white" />
							)}
							<span className="text-white font-medium text-sm">
								{toast.message}
							</span>
						</div>
					</div>,
					document.body,
				)}

			<div className="space-y-6">
				{/* Header with Back Button */}
				<div className="flex items-center space-x-4">
					<Link
						href={
							distribution
								? `/distributions/${distribution.id}`
								: "/distributions"
						}
						className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors cursor-pointer"
					>
						<ArrowLeft className="w-4 h-4" />
						<span className="text-sm font-medium">
							{distribution
								? `Back to ${distribution.name}`
								: "Back to Distributions"}
						</span>
					</Link>
				</div>

				{/* Release Header */}
				<div className="bg-white rounded-lg border border-gray-200 p-4">
					<div className="flex items-center justify-between">
						<div className="flex items-center space-x-3">
							<div className="p-2 bg-gray-100 text-gray-600 rounded">
								<Tag className="w-5 h-5" />
							</div>
							<div className="flex-1">
								<div className="flex items-center space-x-3">
									<h1 className="text-xl font-bold text-gray-900">
										Release {release.version}
									</h1>
									{release.draft && (
										<span className="px-2 py-1 text-xs font-medium bg-yellow-100 text-yellow-800 rounded-full">
											Draft
										</span>
									)}
									{release.yanked && (
										<span className="px-2 py-1 text-xs font-medium bg-red-100 text-red-800 rounded-full">
											Yanked
										</span>
									)}
									{existingDeployment?.status === "InProgress" && (
										<span className="px-2 py-1 text-xs font-medium bg-blue-100 text-blue-800 rounded-full flex items-center">
											<Loader2 className="w-3 h-3 mr-1 animate-spin" />
											Deploying
										</span>
									)}
								</div>
								<div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
									<div className="flex items-center space-x-1">
										<Calendar className="w-4 h-4" />
										<span>
											Created {formatRelativeTime(release.created_at)}
										</span>
									</div>
									{distribution && (
										<div className="flex items-center space-x-2">
											<span className="font-medium">{distribution.name}</span>
											<span
												className={`px-2 py-1 text-xs font-medium rounded-full ${getArchColor(distribution.architecture)}`}
											>
												{distribution.architecture.toUpperCase()}
											</span>
										</div>
									)}
								</div>
							</div>
						</div>
						<div className="flex items-center space-x-3">
							<Link
								href={`/devices?release_id=${release.id}`}
								className="flex items-center space-x-2 px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 rounded-md hover:bg-gray-200 transition-colors cursor-pointer"
							>
								<Cpu className="w-4 h-4" />
								<span>View Devices</span>
							</Link>
							{release?.draft ? (
								<Button
									variant="success"
									loading={updateReleaseHook.isPending}
									icon={<Eye className="w-4 h-4" />}
									onClick={handlePublishRelease}
								>
									{updateReleaseHook.isPending ? "Publishing..." : "Publish"}
								</Button>
							) : (
								!release?.yanked && (
									<>
										<Button
											variant="danger"
											icon={<AlertTriangle className="w-4 h-4" />}
											onClick={() => setShowYankModal(true)}
										>
											Yank
										</Button>
										{existingDeployment?.status === "InProgress" ? (
											<Link
												href={`/releases/${releaseId}/deployment`}
												className="flex items-center space-x-2 px-4 py-2 bg-blue-600 text-white text-sm font-medium rounded-md hover:bg-blue-700 transition-colors cursor-pointer"
											>
												<Loader2 className="w-4 h-4 animate-spin" />
												<span>View Deployment</span>
											</Link>
										) : (
											<Button
												icon={<Rocket className="w-4 h-4" />}
												onClick={handleOpenDeployModal}
											>
												Deploy
											</Button>
										)}
									</>
								)
							)}
						</div>
					</div>
				</div>

				{/* Deploy Confirmation Modal */}
				<Modal
					open={showDeployModal}
					onClose={() => setShowDeployModal(false)}
					title={`Deploy Release v${release.version}`}
					subtitle={
						deployStep === 2
							? `Step 2 of 2 — ${canaryMode === "labels" ? "Select labels for canary devices" : "Select canary devices"}`
							: undefined
					}
					width={deployStep === 1 ? "w-[520px]" : "w-[900px]"}
					footer={
						deployStep === 1 ? (
							<>
								<Button
									variant="secondary"
									disabled={deploying}
									onClick={() => setShowDeployModal(false)}
								>
									Cancel
								</Button>
								{canaryMode === "automatic" ? (
									<Button
										loading={deploying}
										icon={<Rocket className="w-4 h-4" />}
										onClick={handleDeployRelease}
									>
										{deploying ? "Starting..." : "Start Deployment"}
									</Button>
								) : (
									<Button onClick={() => setDeployStep(2)}>
										Next: Select Devices
									</Button>
								)}
							</>
						) : (
							<div className="flex justify-between w-full">
								<Button
									variant="secondary"
									icon={<ArrowLeft className="w-4 h-4" />}
									disabled={deploying}
									onClick={() => setDeployStep(1)}
								>
									Back
								</Button>
								<Button
									loading={deploying}
									icon={<Rocket className="w-4 h-4" />}
									disabled={
										deploying ||
										(canaryMode === "labels" &&
											canaryLabels.length === 0) ||
										(canaryMode === "devices" &&
											canaryDeviceIds.size === 0)
									}
									onClick={handleDeployRelease}
								>
									{deploying ? "Starting..." : "Start Deployment"}
								</Button>
							</div>
						)
					}
				>
					{/* Step 1: Choose canary strategy */}
					{deployStep === 1 && (
						<div className="space-y-4">
							<p className="text-sm text-gray-600">
								First, a small group of canary devices will receive the
								update. After verifying they update successfully, you
								can roll out to all remaining devices.
							</p>

							<div className="space-y-3">
								{(
									[
										{
											mode: "automatic" as const,
											title: "Automatic",
											icon: <Zap className="w-5 h-5 text-amber-500" />,
											description:
												"~10 online devices are automatically selected by network quality. Best for routine deployments.",
										},
										{
											mode: "labels" as const,
											title: "By Labels",
											icon: <Tag className="w-5 h-5 text-blue-500" />,
											description:
												"Target devices matching specific labels (e.g. env=staging). All matching up-to-date devices will be included.",
										},
										{
											mode: "devices" as const,
											title: "Select Devices",
											icon: <Cpu className="w-5 h-5 text-purple-500" />,
											description:
												"Hand-pick specific devices for the canary. Use this when you need precise control.",
										},
									] as const
								).map((option) => (
									<button
										key={option.mode}
										onClick={() => setCanaryMode(option.mode)}
										className={`w-full text-left p-4 rounded-lg border-2 transition-all cursor-pointer ${
											canaryMode === option.mode
												? "border-blue-500 bg-blue-50"
												: "border-gray-200 hover:border-gray-300 hover:bg-gray-50"
										}`}
									>
										<div className="flex items-start gap-3">
											<div className="flex-shrink-0 mt-0.5">
												{option.icon}
											</div>
											<div>
												<div className="font-medium text-gray-900 text-sm">
													{option.title}
												</div>
												<p className="text-xs text-gray-500 mt-1">
													{option.description}
												</p>
											</div>
										</div>
									</button>
								))}
							</div>
						</div>
					)}

					{/* Step 2: Select & Preview */}
					{deployStep === 2 && (
						<div className="-m-6 flex-1 overflow-hidden grid grid-cols-2 divide-x divide-gray-200 min-h-[400px]">
							{/* Left column: selection controls */}
							<div className="p-6 overflow-y-auto space-y-3">
								{canaryMode === "labels" && (
									<div className="space-y-3">
										<LabelAutocomplete
											onSelect={(label) =>
												setCanaryLabels((prev) => [...prev, label])
											}
											existingFilters={canaryLabels}
										/>
										{canaryLabels.length > 0 && (
											<div className="flex flex-wrap gap-1">
												{canaryLabels.map((label) => (
													<span
														key={label}
														className="inline-flex items-center px-2 py-0.5 text-xs font-medium bg-blue-100 text-blue-800 rounded-full"
													>
														{label}
														<button
															onClick={() =>
																setCanaryLabels((prev) =>
																	prev.filter((l) => l !== label),
																)
															}
															className="ml-1 text-blue-600 hover:text-blue-900 cursor-pointer"
														>
															<X className="w-3 h-3" />
														</button>
													</span>
												))}
											</div>
										)}
										<p className="text-xs text-gray-500">
											All up-to-date devices matching these labels will
											be included in the canary.
										</p>
									</div>
								)}

								{canaryMode === "devices" && (
									<div className="space-y-2">
										<div className="relative">
											<Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
											<input
												type="text"
												placeholder="Search by serial number..."
												value={deviceSearchQuery}
												onChange={(e) =>
													setDeviceSearchQuery(e.target.value)
												}
												className="w-full pl-8 pr-3 py-1.5 text-sm border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 placeholder-gray-400 bg-white"
											/>
										</div>
										<div className="max-h-[400px] overflow-y-auto border border-gray-200 rounded-md">
											{eligibleDevicesLoading ? (
												<div className="p-4 text-center text-sm text-gray-500">
													Loading devices...
												</div>
											) : filteredEligibleDevices.length === 0 ? (
												<div className="p-4 text-center text-sm text-gray-500">
													No eligible devices found
												</div>
											) : (
												filteredEligibleDevices.map(
													(device: Device) => {
														const isSelected = canaryDeviceIds.has(
															device.id,
														);
														return (
															<div
																key={device.id}
																onClick={() =>
																	setCanaryDeviceIds((prev) => {
																		const next = new Set(prev);
																		if (next.has(device.id)) {
																			next.delete(device.id);
																		} else {
																			next.add(device.id);
																		}
																		return next;
																	})
																}
																className={`flex items-center gap-3 px-3 py-2.5 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
																	isSelected
																		? "bg-blue-50"
																		: "hover:bg-gray-50"
																}`}
															>
																<div
																	className={`w-5 h-5 rounded-full flex items-center justify-center flex-shrink-0 transition-colors ${
																		isSelected
																			? "bg-blue-600"
																			: "border-2 border-gray-300"
																	}`}
																>
																	{isSelected && (
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
																</div>
																<span className="text-sm font-medium text-gray-900 truncate flex-shrink-0">
																	{device.serial_number}
																</span>
																{device.labels &&
																	Object.keys(device.labels).length >
																		0 && (
																		<div className="flex flex-wrap gap-1 min-w-0">
																			{Object.entries(
																				device.labels,
																			).map(([key, value]) => (
																				<code
																					key={key}
																					className="px-1 py-0.5 text-[10px] font-mono rounded border bg-gray-100 text-gray-600 border-gray-200"
																				>
																					{key}={value}
																				</code>
																			))}
																		</div>
																	)}
															</div>
														);
													},
												)
											)}
										</div>
									</div>
								)}
							</div>

							{/* Right column: live preview */}
							<div className="p-6 overflow-y-auto bg-gray-50 flex flex-col">
								{canaryMode === "labels" &&
									(canaryLabels.length === 0 ? (
										<>
											<h4 className="font-medium text-gray-900 text-sm mb-1">
												Canary Preview
											</h4>
											<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
												Select labels to see matching devices
											</div>
										</>
									) : labelDevicesLoading ? (
										<>
											<h4 className="font-medium text-gray-900 text-sm mb-3">
												Canary Preview
											</h4>
											<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
												Loading...
											</div>
										</>
									) : labelMatchDevices.length === 0 ? (
										<>
											<h4 className="font-medium text-gray-900 text-sm mb-1">
												0 devices will receive the canary update
											</h4>
											<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
												No matching devices
											</div>
										</>
									) : (
										<>
											<h4 className="font-medium text-gray-900 text-sm mb-3">
												{labelMatchDevices.length} device
												{labelMatchDevices.length !== 1 ? "s" : ""} will
												receive the canary update
											</h4>
											<div className="border border-gray-200 rounded-md bg-white overflow-y-auto flex-1">
												{labelMatchDevices.map((device: Device) => (
													<div
														key={device.id}
														className="flex items-center gap-2 px-3 py-2.5 border-b border-gray-100 last:border-b-0"
													>
														<span className="text-sm font-mono text-gray-900 flex-shrink-0">
															{device.serial_number}
														</span>
														{device.labels &&
															Object.keys(device.labels).length > 0 && (
																<div className="flex flex-wrap gap-1 min-w-0">
																	{Object.entries(device.labels).map(
																		([key, value]) => (
																			<code
																				key={key}
																				className="px-1.5 py-0.5 text-[10px] font-mono rounded border bg-gray-100 text-gray-700 border-gray-200"
																			>
																				{key}={value}
																			</code>
																		),
																	)}
																</div>
															)}
													</div>
												))}
											</div>
										</>
									))}

								{canaryMode === "devices" &&
									(canaryDeviceIds.size === 0 ? (
										<>
											<h4 className="font-medium text-gray-900 text-sm mb-1">
												Canary Preview
											</h4>
											<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
												Select devices from the list
											</div>
										</>
									) : (
										<>
											<h4 className="font-medium text-gray-900 text-sm mb-3">
												{canaryDeviceIds.size} device
												{canaryDeviceIds.size !== 1 ? "s" : ""} will
												receive the canary update
											</h4>
											<div className="border border-gray-200 rounded-md bg-white overflow-y-auto flex-1">
												{eligibleDevices
													.filter((d: Device) =>
														canaryDeviceIds.has(d.id),
													)
													.map((device: Device) => (
														<div
															key={device.id}
															className="flex items-center gap-2 px-3 py-2.5 border-b border-gray-100 last:border-b-0"
														>
															<span className="text-sm font-mono text-gray-900 flex-shrink-0">
																{device.serial_number}
															</span>
															{device.labels &&
																Object.keys(device.labels).length >
																	0 && (
																	<div className="flex flex-wrap gap-1 min-w-0">
																		{Object.entries(device.labels).map(
																			([key, value]) => (
																				<code
																					key={key}
																					className="px-1.5 py-0.5 text-[10px] font-mono rounded border bg-gray-100 text-gray-700 border-gray-200"
																				>
																					{key}={value}
																				</code>
																			),
																		)}
																	</div>
																)}
															<button
																onClick={() =>
																	setCanaryDeviceIds((prev) => {
																		const next = new Set(prev);
																		next.delete(device.id);
																		return next;
																	})
																}
																className="text-gray-400 hover:text-red-500 cursor-pointer ml-auto flex-shrink-0"
															>
																<X className="w-3.5 h-3.5" />
															</button>
														</div>
													))}
											</div>
										</>
									))}
							</div>
						</div>
					)}
				</Modal>

				{/* Yank Release Modal */}
				<Modal
					open={showYankModal}
					onClose={() => {
						setShowYankModal(false);
						setYankReason("");
					}}
					title={`Yank Release v${release.version}`}
					width="w-[480px]"
					footer={
						<>
							<Button
								variant="secondary"
								disabled={yanking}
								onClick={() => {
									setShowYankModal(false);
									setYankReason("");
								}}
							>
								Cancel
							</Button>
							<Button
								variant="danger"
								loading={yanking}
								disabled={!yankReason.trim()}
								icon={<AlertTriangle className="w-4 h-4" />}
								onClick={handleYankRelease}
							>
								{yanking ? "Yanking..." : "Yank Release"}
							</Button>
						</>
					}
				>
					<div className="space-y-4">
						<div className="bg-red-50 border border-red-200 rounded-lg p-4">
							<div className="flex items-start space-x-3">
								<AlertTriangle className="w-5 h-5 text-red-600 flex-shrink-0 mt-0.5" />
								<div>
									<h4 className="font-medium text-red-900 text-sm mb-1">
										Warning
									</h4>
									<p className="text-sm text-red-800">
										Yanking a release will prevent devices from updating
										to this version. This action cannot be undone.
									</p>
								</div>
							</div>
						</div>

						<div>
							<label className="block text-sm font-medium text-gray-700 mb-2">
								Reason for yanking
							</label>
							<input
								type="text"
								placeholder="e.g., Critical bug discovered"
								value={yankReason}
								onChange={(e) => setYankReason(e.target.value)}
								className="w-full px-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-red-500 focus:border-red-500 placeholder:text-gray-400"
							/>
						</div>
					</div>
				</Modal>

				{/* Replace Package Modal */}
				{packageToReplace && (
					<Modal
						open={showReplacePackageModal}
						onClose={() => setShowReplacePackageModal(false)}
						title={`Replace Package: ${packageToReplace.name}`}
						width="w-[640px]"
						footer={
							<>
								<Button
									variant="secondary"
									onClick={() => setShowReplacePackageModal(false)}
								>
									Cancel
								</Button>
								<Button
									disabled={!selectedAvailablePackage}
									onClick={handleReplacePackage}
								>
									Replace Version
								</Button>
							</>
						}
					>
						<div className="mb-4 p-3 bg-gray-50 rounded-md">
							<div className="text-sm">
								<div className="font-medium text-gray-900">
									Current Version
								</div>
								<div className="text-gray-600 mt-1">
									{packageToReplace.name} v{packageToReplace.version}
								</div>
							</div>
						</div>

						<div className="space-y-4">
							<div>
								<label className="block text-sm font-medium text-gray-700 mb-2">
									Search Versions
								</label>
								<div className="relative">
									<Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
									<input
										type="text"
										placeholder="Search by version..."
										value={packageSearchQuery}
										onChange={(e) => setPackageSearchQuery(e.target.value)}
										className="w-full pl-10 pr-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-blue-500 focus:border-blue-500 placeholder:text-gray-400"
									/>
								</div>
							</div>

							<div>
								<label className="block text-sm font-medium text-gray-700 mb-2">
									Available Versions
								</label>
								{availablePackagesLoading ? (
									<div className="flex items-center justify-center py-8">
										<div className="text-gray-500 text-sm">
											Loading available versions...
										</div>
									</div>
								) : (
									(() => {
										const filteredPackages = availablePackages
											.filter(
												(pkg) =>
													pkg.name === packageToReplace.name &&
													pkg.id !== packageToReplace.id &&
													(!distribution ||
														pkg.architecture ===
															distribution.architecture) &&
													(packageSearchQuery === "" ||
														pkg.version
															.toLowerCase()
															.includes(packageSearchQuery.toLowerCase())),
											)
											.sort(
												(a, b) =>
													new Date(b.created_at).getTime() -
													new Date(a.created_at).getTime(),
											);

										return filteredPackages.length === 0 ? (
											<div className="text-center py-8 text-gray-500 text-sm">
												{packageSearchQuery
													? "No versions match your search"
													: "No other versions available"}
											</div>
										) : (
											<div className="border border-gray-200 rounded-md max-h-[320px] overflow-y-auto">
												{filteredPackages.map((pkg) => (
													<button
														key={pkg.id}
														onClick={() =>
															setSelectedAvailablePackage(pkg.id)
														}
														className={`w-full text-left p-3 border-b border-gray-200 last:border-b-0 hover:bg-gray-50 transition-colors cursor-pointer ${
															selectedAvailablePackage === pkg.id
																? "bg-blue-50 hover:bg-blue-50"
																: ""
														}`}
													>
														<div className="flex items-start justify-between">
															<div className="flex-1 min-w-0">
																<div className="flex items-center space-x-2">
																	<span className="font-medium text-gray-900">
																		{pkg.name}
																	</span>
																	<span className="text-xs text-gray-500">
																		v{pkg.version}
																	</span>
																	<span
																		className={`px-2 py-0.5 text-xs font-medium rounded ${getArchColor(pkg.architecture)}`}
																	>
																		{pkg.architecture}
																	</span>
																</div>
																<div className="text-xs text-gray-500 mt-1 truncate">
																	{pkg.file}
																</div>
															</div>
															{selectedAvailablePackage === pkg.id && (
																<div className="flex-shrink-0 ml-2">
																	<div className="w-5 h-5 bg-blue-600 rounded-full flex items-center justify-center">
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
												))}
											</div>
										);
									})()
								)}
							</div>
						</div>
					</Modal>
				)}

				{/* Add Package Modal */}
				<Modal
					open={showAddPackageModal}
					onClose={() => setShowAddPackageModal(false)}
					title="Add Package to Release"
					width="w-[640px]"
					footer={
						<>
							<Button
								variant="secondary"
								onClick={() => setShowAddPackageModal(false)}
							>
								Cancel
							</Button>
							<Button
								disabled={!selectedAvailablePackage}
								onClick={handleAddPackage}
							>
								Add Package
							</Button>
						</>
					}
				>
					<div className="space-y-4">
						<div>
							<label className="block text-sm font-medium text-gray-700 mb-2">
								Search Packages
							</label>
							<div className="relative">
								<Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
								<input
									type="text"
									placeholder="Search by name or version..."
									value={packageSearchQuery}
									onChange={(e) => setPackageSearchQuery(e.target.value)}
									className="w-full pl-10 pr-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-blue-500 focus:border-blue-500 placeholder:text-gray-400"
								/>
							</div>
						</div>

						<div>
							<label className="block text-sm font-medium text-gray-700 mb-2">
								Available Packages
							</label>
							{availablePackagesLoading ? (
								<div className="flex items-center justify-center py-8">
									<div className="text-gray-500 text-sm">
										Loading available packages...
									</div>
								</div>
							) : (
								(() => {
									const packagesByName = new Map<string, Package>();

									availablePackages
										.filter(
											(pkg) =>
												!packages.some(
													(releasePkg) => releasePkg.name === pkg.name,
												) &&
												(!distribution ||
													pkg.architecture === distribution.architecture),
										)
										.forEach((pkg) => {
											const existing = packagesByName.get(pkg.name);
											if (
												!existing ||
												compareVersions(pkg.version, existing.version) > 0
											) {
												packagesByName.set(pkg.name, pkg);
											}
										});

									const filteredPackages = Array.from(
										packagesByName.values(),
									)
										.filter(
											(pkg) =>
												packageSearchQuery === "" ||
												pkg.name
													.toLowerCase()
													.includes(packageSearchQuery.toLowerCase()) ||
												pkg.version
													.toLowerCase()
													.includes(packageSearchQuery.toLowerCase()),
										)
										.sort((a, b) => a.name.localeCompare(b.name));

									return filteredPackages.length === 0 ? (
										<div className="text-center py-8 text-gray-500 text-sm">
											{packageSearchQuery
												? "No packages match your search"
												: "No available packages"}
										</div>
									) : (
										<div className="border border-gray-200 rounded-md max-h-[320px] overflow-y-auto">
											{filteredPackages.map((pkg) => (
												<button
													key={pkg.id}
													onClick={() =>
														setSelectedAvailablePackage(pkg.id)
													}
													className={`w-full text-left p-3 border-b border-gray-200 last:border-b-0 hover:bg-gray-50 transition-colors cursor-pointer ${
														selectedAvailablePackage === pkg.id
															? "bg-blue-50 hover:bg-blue-50"
															: ""
													}`}
												>
													<div className="flex items-start justify-between">
														<div className="flex-1 min-w-0">
															<div className="flex items-center space-x-2">
																<span className="font-medium text-gray-900">
																	{pkg.name}
																</span>
																<span className="text-xs text-gray-500">
																	v{pkg.version}
																</span>
																<span
																	className={`px-2 py-0.5 text-xs font-medium rounded ${getArchColor(pkg.architecture)}`}
																>
																	{pkg.architecture}
																</span>
															</div>
															<div className="text-xs text-gray-500 mt-1 truncate">
																{pkg.file}
															</div>
														</div>
														{selectedAvailablePackage === pkg.id && (
															<div className="flex-shrink-0 ml-2">
																<div className="w-5 h-5 bg-blue-600 rounded-full flex items-center justify-center">
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
											))}
										</div>
									);
								})()
							)}
						</div>
					</div>
				</Modal>

				{/* Packages and Services Grid */}
				<div className="grid grid-cols-2 gap-6">
					{/* Packages Section */}
					<div className="space-y-4">
						<div className="flex items-center justify-between">
							<div className="flex items-center space-x-2">
								<Box className="w-5 h-5 text-gray-600" />
								<h2 className="text-lg font-semibold text-gray-900">
									Packages
								</h2>
								<span className="text-sm text-gray-500">
									({packages.length})
								</span>
							</div>
							{release?.draft && (
								<Button
									icon={<Plus className="w-4 h-4" />}
									onClick={openAddModal}
								>
									Add
								</Button>
							)}
						</div>

						<div className="bg-white rounded border border-gray-200 overflow-hidden">
							{packagesLoading ? (
								<div className="p-6 text-center">
									<div className="text-gray-500 text-sm">
										Loading packages...
									</div>
								</div>
							) : packages.length === 0 ? (
								<div className="p-6 text-center">
									<Box className="w-8 h-8 text-gray-400 mx-auto mb-2" />
									<p className="text-sm text-gray-500">No packages found</p>
								</div>
							) : (
								<div className="divide-y divide-gray-200">
									{packages.map((pkg) => {
										const latestVersion = getLatestVersionForPackage(pkg);
										const isUpgrading = upgradingPackages.has(pkg.id);
										return (
											<div
												key={pkg.id}
												className="p-4 hover:bg-gray-50 transition-colors"
											>
												<div className="flex items-center justify-between">
													<div className="flex items-center space-x-3 min-w-0">
														<div className="p-2 bg-gray-100 text-gray-600 rounded flex-shrink-0">
															<PackageIcon className="w-4 h-4" />
														</div>
														<div className="min-w-0">
															<div className="flex items-center space-x-2">
																<span className="font-medium text-gray-900 truncate">
																	{pkg.name}
																</span>
																<span className="text-xs text-gray-500 flex-shrink-0">
																	v{pkg.version}
																</span>
																{release?.draft && latestVersion && (
																	<button
																		onClick={() => handleUpgradePackage(pkg)}
																		disabled={isUpgrading}
																		className={`flex items-center space-x-0.5 px-1.5 py-0.5 text-[10px] font-medium rounded-md transition-all ${
																			isUpgrading
																				? "bg-gray-200 text-gray-400 cursor-not-allowed"
																				: "bg-green-100 text-green-700 hover:bg-green-200 cursor-pointer"
																		}`}
																		title={`Upgrade to v${latestVersion.version}`}
																	>
																		{isUpgrading ? (
																			<div className="w-2.5 h-2.5 border-2 border-gray-400 border-t-transparent rounded-full animate-spin"></div>
																		) : (
																			<>
																				<ArrowUp className="w-2.5 h-2.5" />
																				<span>v{latestVersion.version}</span>
																			</>
																		)}
																	</button>
																)}
															</div>
														</div>
													</div>
													{release?.draft && (
														<div className="flex items-center space-x-1 flex-shrink-0">
															<IconButton
																icon={<ChevronsUpDown className="w-4 h-4" />}
																onClick={() => openReplaceModal(pkg)}
																title="Select different version"
															/>
															<IconButton
																icon={<Trash2 className="w-4 h-4" />}
																onClick={() => handleDeletePackage(pkg.id)}
																className="hover:text-red-600 hover:bg-red-50"
																title="Remove package"
															/>
														</div>
													)}
												</div>
											</div>
										);
									})}
								</div>
							)}
						</div>
					</div>

					{/* Services Section */}
					<div className="space-y-4">
						<div className="flex items-center space-x-2">
							<Cog className="w-5 h-5 text-gray-600" />
							<h2 className="text-lg font-semibold text-gray-900">Services</h2>
							<span className="text-sm text-gray-500">({services.length})</span>
						</div>

						<div className="bg-white rounded border border-gray-200 overflow-hidden">
							{servicesLoading ? (
								<div className="p-6 text-center">
									<div className="text-gray-500 text-sm">
										Loading services...
									</div>
								</div>
							) : services.length === 0 ? (
								<div className="p-6 text-center">
									<Cog className="w-8 h-8 text-gray-400 mx-auto mb-2" />
									<p className="text-sm text-gray-500">No services found</p>
									<p className="text-xs text-gray-400 mt-1">
										Extracted from .deb packages
									</p>
								</div>
							) : (
								<div className="divide-y divide-gray-200">
									{services.map((service) => {
										const packageName = getPackageNameForService(
											service.package_id,
										);
										return (
											<div
												key={service.id}
												className="p-4 hover:bg-gray-50 transition-colors"
											>
												<div className="flex items-center space-x-3">
													<div className="p-2 bg-purple-100 text-purple-600 rounded flex-shrink-0">
														<Cog className="w-4 h-4" />
													</div>
													<div className="min-w-0">
														<div className="flex items-center space-x-2">
															<span className="font-medium text-gray-900 truncate">
																{service.service_name}
															</span>
															{service.watchdog_sec && (
																<span className="flex items-center space-x-1 px-2 py-0.5 text-xs font-medium bg-blue-100 text-blue-700 rounded-full flex-shrink-0">
																	<Clock className="w-3 h-3" />
																	<span>{service.watchdog_sec}s</span>
																</span>
															)}
														</div>
														{packageName && (
															<p className="text-xs text-gray-500 mt-1">
																From: {packageName}
															</p>
														)}
													</div>
												</div>
											</div>
										);
									})}
								</div>
							)}
						</div>
					</div>
				</div>
			</div>
		</>
	);
};

export default ReleaseDetailPage;
