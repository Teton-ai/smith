import { useQueryClient } from "@tanstack/react-query";
import {
	BackLink,
	Badge,
	type BadgeVariant,
	Button,
	Card,
	ListRow,
	PageContainer,
	Panel,
	SECTION_THEMES,
	SectionCard,
	Toast,
	type ToastState,
} from "@teton/smith-ui";
import {
	AlertTriangle,
	ArrowLeft,
	ArrowUp,
	ArrowUpDown,
	Box,
	Calendar,
	ChevronsUpDown,
	Clock,
	Cog,
	Cpu,
	Download,
	Eye,
	HardDrive,
	Loader2,
	Package as PackageIcon,
	Plus,
	Rocket,
	RotateCcw,
	Search,
	ShieldCheck,
	Tag,
	Trash2,
	X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import {
	isActive,
	PackageChanges,
	PreflightNotice,
	ProgressBar,
	preflightQueryKey,
	StateBadge,
	usePreflight,
} from "@/app/(private)/deployments/shared";
import {
	type DeploymentRequest,
	type Device,
	getGetOsQueryKey,
	type Package,
	type Release,
	useAddPackageToRelease,
	useApiListDeployments,
	useApiReleaseDeployment,
	useApiRollbackRelease,
	useDeleteOs,
	useDeletePackageForRelease,
	useDownloadOsHook,
	useGetDevices,
	useGetDistributionById,
	useGetDistributionLatestRelease,
	useGetDistributionReleasePackages,
	useGetDistributionReleases,
	useGetOs,
	useGetPackages,
	useGetRelease,
	useGetReleaseServices,
	usePatchRelease,
} from "@/app/api-client";
import LabelAutocomplete from "@/app/components/LabelAutocomplete";
import { Modal } from "@/app/components/modal";
import { RelativeTime } from "@/app/components/RelativeTime";
import { humanBytes } from "@/app/utils/format";
import {
	defaultRollbackTarget,
	requestErrorMessage,
	rollbackCandidates,
} from "@/app/utils/release";

const getArchVariant = (architecture: string): BadgeVariant => {
	switch (architecture.toLowerCase()) {
		case "x86_64":
		case "amd64":
			return "blue";
		case "arm64":
		case "aarch64":
			return "green";
		case "armv7":
		case "arm":
			return "purple";
		default:
			return "gray";
	}
};

/** How a pilot group of `count` devices will be deployed to. */
const pilotGroupHeading = (count: number) =>
	`${count} device${count !== 1 ? "s" : ""} in the pilot group: ${
		count < 10
			? "they update directly, without a canary"
			: "up to 10 of them are the canary"
	}`;

const ReleaseDetailPage = () => {
	const navigate = useNavigate();
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
	// A release candidate's pilot group; a stable release's canary is always automatic.
	const [pilotMode, setPilotMode] = useState<"labels" | "devices">("labels");
	const [pilotLabels, setPilotLabels] = useState<string[]>([]);
	const [pilotDeviceIds, setPilotDeviceIds] = useState<Map<number, Device>>(
		new Map(),
	);
	const [deviceSearchQuery, setDeviceSearchQuery] = useState("");
	const [debouncedDeviceSearchQuery, setDebouncedDeviceSearchQuery] =
		useState("");
	const [deployStep, setDeployStep] = useState<1 | 2>(1);
	const [deployError, setDeployError] = useState<string | null>(null);
	const [publishError, setPublishError] = useState<string | null>(null);
	const [showYankModal, setShowYankModal] = useState(false);
	const [yanking, setYanking] = useState(false);
	const [yankReason, setYankReason] = useState("");
	const [rollbackAfterYank, setRollbackAfterYank] = useState(true);
	const [showRollbackModal, setShowRollbackModal] = useState(false);
	const [rollbackTargetId, setRollbackTargetId] = useState<number | null>(null);
	const [toast, setToast] = useState<ToastState | null>(null);
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

	// One request feeds both the header badge and the history list; the API
	// returns newest first, so [0] is the latest.
	const {
		data: releaseDeployments = [],
		isLoading: deploymentsLoading,
		isError: deploymentsError,
		queryKey: deploymentsQueryKey,
	} = useApiListDeployments(
		{ release_id: releaseId, limit: 200 },
		{ query: { enabled: !!release && !release.draft, refetchInterval: 10000 } },
	);
	// The rollback that moved devices off this release, if it was yanked.
	const { data: rollbacks = [], queryKey: rollbacksQueryKey } =
		useApiListDeployments(
			{ rolled_back_from: releaseId, limit: 1 },
			{ query: { enabled: !!release?.yanked } },
		);
	const rollback = rollbacks[0];
	const lastDeployment = releaseDeployments[0];
	const activeDeployment =
		lastDeployment && isActive(lastDeployment.state)
			? lastDeployment
			: undefined;

	// On a draft this says what publishing would change; in the deploy dialog,
	// what a fleet rollout would.
	const publishCheck = usePreflight(
		releaseId,
		{ publish: true },
		!!release?.draft,
	);
	const deployCheck = usePreflight(
		releaseId,
		{ publish: false },
		showDeployModal,
	);

	const rollbackPickerOpen = showYankModal || showRollbackModal;
	const { data: distributionReleases = [] } = useGetDistributionReleases(
		release?.distribution_id as number,
		undefined,
		{
			query: { enabled: rollbackPickerOpen && !!release?.distribution_id },
		},
	);
	const { data: latestRelease } = useGetDistributionLatestRelease(
		release?.distribution_id as number,
		{
			query: {
				enabled: rollbackPickerOpen && !!release?.distribution_id,
				retry: false,
			},
		},
	);
	const rollbackOptions = useMemo(
		() => (release ? rollbackCandidates(distributionReleases, release) : []),
		[distributionReleases, release],
	);
	const rollbackTarget: Release | undefined =
		rollbackOptions.find((r) => r.id === rollbackTargetId) ??
		(release
			? defaultRollbackTarget(rollbackOptions, release, latestRelease)
			: undefined);

	// Eligible devices: online + up-to-date in distribution (used for devices mode)
	const { data: eligibleDevices = [], isLoading: eligibleDevicesLoading } =
		useGetDevices(
			{
				distribution_id: release?.distribution_id,
				search: debouncedDeviceSearchQuery.trim() || undefined,
				limit: 1000,
			},
			{
				query: {
					enabled:
						showDeployModal &&
						pilotMode === "devices" &&
						!!release?.distribution_id,
				},
			},
		);

	// Devices matching selected labels (used for labels mode preview)
	const { data: labelMatchDevices = [], isLoading: labelDevicesLoading } =
		useGetDevices(
			{
				distribution_id: release?.distribution_id,
				labels: pilotLabels,
			},
			{
				query: {
					enabled:
						showDeployModal &&
						pilotMode === "labels" &&
						pilotLabels.length > 0 &&
						!!release?.distribution_id,
				},
			},
		);

	// Devices with unhealthy services in this distribution
	const { data: unhealthyDevices = [] } = useGetDevices(
		{
			distribution_id: release?.distribution_id,
			service_not_running: true,
		},
		{
			query: {
				enabled:
					showDeployModal && deployStep === 2 && !!release?.distribution_id,
			},
		},
	);

	const unhealthyDeviceIds = useMemo(
		() => new Set(unhealthyDevices.map((d: Device) => d.id)),
		[unhealthyDevices],
	);

	const networkDotColor = (score?: number) => {
		if (score == null) return "bg-gray-300";
		if (score >= 4) return "bg-green-500";
		if (score >= 2) return "bg-yellow-500";
		return "bg-red-500";
	};

	const networkTooltip = (score?: number) => {
		if (score == null) return "No network data";
		return `Network score: ${score}/5`;
	};

	const renderDeviceIndicators = (device: Device) => (
		<div className="flex items-center gap-1.5 flex-shrink-0">
			<span
				title={networkTooltip(device.network?.network_score)}
				className={`inline-block w-2 h-2 rounded-full ${networkDotColor(device.network?.network_score)}`}
			/>
			{unhealthyDeviceIds.has(device.id) && (
				<span title="Unhealthy services" className="inline-flex">
					<AlertTriangle className="w-3 h-3 text-amber-500" />
				</span>
			)}
		</div>
	);

	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	useEffect(() => {
		const timer = setTimeout(() => {
			setDebouncedDeviceSearchQuery(deviceSearchQuery);
		}, 300);
		return () => clearTimeout(timer);
	}, [deviceSearchQuery]);

	const updateReleaseHook = usePatchRelease({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: releaseQueryKey });
			},
		},
	});
	const handlePublishRelease = async () => {
		setPublishError(null);
		updateReleaseHook.mutate(
			{ releaseId, data: { draft: false } },
			{ onError: (error) => setPublishError(requestErrorMessage(error)) },
		);
	};
	const handleToggleLts = async () => {
		updateReleaseHook.mutate({ releaseId, data: { lts: !release?.lts } });
	};

	// The publish and deploy previews compare this release's packages, so they
	// go stale with the list.
	const refreshPackages = () => {
		queryClient.invalidateQueries({ queryKey: packagesQueryKey });
		queryClient.invalidateQueries({ queryKey: preflightQueryKey(releaseId) });
	};

	const addPackageToReleaseHook = useAddPackageToRelease({
		mutation: {
			onSuccess: () => {
				refreshPackages();
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
				refreshPackages();
			},
		},
	});
	const handleDeletePackage = async (packageId: number) => {
		if (!confirm("Are you sure you want to delete this package?")) return;
		deletePackageForReleaseHook.mutate({ releaseId, packageId });
	};

	const osQueryKey = getGetOsQueryKey(releaseId);
	// A release without an image 404s, which is the ordinary case rather than a
	// failure, so it is not retried and an error is read as "no image".
	const { data: os, isLoading: osLoading } = useGetOs(releaseId, {
		query: { enabled: !!releaseId, retry: false },
	});
	const downloadOs = useDownloadOsHook();
	const [downloadingOs, setDownloadingOs] = useState(false);
	const deleteOsHook = useDeleteOs({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: osQueryKey });
			},
		},
	});

	const handleDownloadOs = async () => {
		setDownloadingOs(true);
		try {
			const ready = await downloadOs(releaseId);
			// The link points straight at the CDN, so the browser's own download
			// manager owns a multi-gigabyte transfer — nothing buffers in the tab.
			// `download` is ignored for a cross-origin href, so it only documents
			// intent: the saved name comes from the Content-Disposition the api
			// stamps on the object at upload.
			const anchor = document.createElement("a");
			anchor.href = ready.url;
			anchor.download = ready.file_name;
			document.body.appendChild(anchor);
			anchor.click();
			anchor.remove();
			setToast({
				message: `Downloading ${ready.file_name} (${humanBytes(ready.size_bytes)})`,
				type: "success",
			});
		} catch (error: any) {
			setToast({
				message: `Failed to get download link: ${error?.message || "Unknown error"}`,
				type: "error",
			});
		} finally {
			setDownloadingOs(false);
		}
	};

	const handleDeleteOs = () => {
		if (!confirm("Are you sure you want to delete this OS image?")) return;
		deleteOsHook.mutate({ releaseId });
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

			refreshPackages();
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

			refreshPackages();

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
		setPilotMode("labels");
		setPilotLabels([]);
		setPilotDeviceIds(new Map());
		setDeviceSearchQuery("");
		setDeployStep(1);
		setDeployError(null);
		setShowDeployModal(true);
	};

	const releaseDeploymentHook = useApiReleaseDeployment();
	const deployBlocked =
		deployCheck.isLoading ||
		deployCheck.data?.allowed === false ||
		!!deployCheck.data?.active;

	const handleDeployRelease = async () => {
		if (!release || deploying) return;

		let data: DeploymentRequest = {};
		if (release.release_candidate && pilotMode === "labels") {
			data = { pilot_device_labels: pilotLabels };
		} else if (release.release_candidate && pilotMode === "devices") {
			data = { pilot_device_ids: Array.from(pilotDeviceIds.keys()) };
		}

		setDeploying(true);
		try {
			const deployment = await releaseDeploymentHook.mutateAsync({
				releaseId,
				data,
			});
			queryClient.invalidateQueries({ queryKey: deploymentsQueryKey });
			setShowDeployModal(false);
			navigate(`/deployments/${deployment.id}`);
		} catch (error: unknown) {
			console.error("Failed to deploy release:", error);
			setDeployError(requestErrorMessage(error));
		} finally {
			setDeploying(false);
		}
	};

	const rollbackReleaseHook = useApiRollbackRelease({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: releaseQueryKey });
			},
		},
	});

	const rollBackDevices = async (target: Release, yankReason?: string) => {
		const result = await rollbackReleaseHook.mutateAsync({
			releaseId,
			data: { target_release_id: target.id, yank_reason: yankReason },
		});
		queryClient.invalidateQueries({ queryKey: rollbacksQueryKey });
		return `${result.devices_retargeted} device(s) rolled back to v${target.version}`;
	};

	const closeYankModal = () => {
		setShowYankModal(false);
		setYankReason("");
		setRollbackAfterYank(true);
		setRollbackTargetId(null);
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

		const target = rollbackAfterYank ? rollbackTarget : undefined;
		const reason = yankReason.trim();
		setYanking(true);
		try {
			// One request when rolling back: the rollback yanks the release in the same
			// transaction, so devices still heading to it go to the target too.
			if (target) {
				const summary = await rollBackDevices(target, reason);
				setToast({ message: `Release yanked, ${summary}`, type: "success" });
			} else {
				await updateReleaseHook.mutateAsync({
					releaseId,
					data: { yanked: true, yanked_reason: reason },
				});
				setToast({ message: "Release has been yanked", type: "success" });
			}
			closeYankModal();
		} catch (error) {
			setToast({
				message: `Failed to yank release: ${requestErrorMessage(error)}`,
				type: "error",
			});
		} finally {
			setYanking(false);
		}
	};

	const handleRollback = async () => {
		if (!rollbackTarget || rollbackReleaseHook.isPending) return;
		try {
			const summary = await rollBackDevices(rollbackTarget);
			setShowRollbackModal(false);
			setRollbackTargetId(null);
			setToast({ message: summary, type: "success" });
		} catch (error) {
			console.error("Failed to roll back release:", error);
			setToast({
				message: `Rollback failed: ${requestErrorMessage(error)}`,
				type: "error",
			});
		}
	};

	const renderRollbackTargetSelect = () =>
		rollbackOptions.length === 0 ? (
			<p className="text-sm text-gray-500">
				No other stable release in this distribution to roll back to.
			</p>
		) : (
			<select
				value={rollbackTarget?.id ?? ""}
				onChange={(e) => setRollbackTargetId(Number(e.target.value))}
				className="w-full px-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-red-500 focus:border-red-500"
			>
				{rollbackOptions.map((r) => (
					<option key={r.id} value={r.id}>
						v{r.version}
						{r.id === latestRelease?.id ? " (latest)" : ""}
						{r.lts ? " (LTS)" : ""}
					</option>
				))}
			</select>
		);

	const getPackageNameForService = (packageId?: number): string | null => {
		if (packageId == null) return null;
		const pkg = packages.find((p) => p.id === packageId);
		return pkg ? `${pkg.name} v${pkg.version}` : null;
	};

	if (loading || !release) {
		return (
			<PageContainer>
				<div className="h-4 w-44 bg-gray-200 rounded animate-pulse" />
				<Card className="p-4">
					<div className="flex items-center space-x-3">
						<div className="p-2 bg-gray-100 rounded">
							<div className="w-5 h-5 bg-gray-200 rounded animate-pulse" />
						</div>
						<div className="space-y-2">
							<div className="h-6 bg-gray-200 rounded w-48 animate-pulse" />
							<div className="h-4 bg-gray-200 rounded w-64 animate-pulse" />
						</div>
					</div>
				</Card>
			</PageContainer>
		);
	}

	return (
		<PageContainer>
			{/* Toast Notification */}
			<Toast toast={toast} onClose={() => setToast(null)} />

			{/* Back link */}
			<BackLink
				to={
					distribution ? `/distributions/${distribution.id}` : "/distributions"
				}
			>
				{distribution
					? `Back to ${distribution.name}`
					: "Back to Distributions"}
			</BackLink>

			{/* Release Header */}
			<Card className="p-4">
				<div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
					<div className="flex items-center space-x-3 min-w-0">
						<div className="p-2 bg-gray-100 text-gray-600 rounded flex-shrink-0">
							<Tag className="w-5 h-5" />
						</div>
						<div className="min-w-0">
							<div className="flex items-center space-x-3">
								<h1 className="text-xl font-bold text-gray-900">
									Release {release.version}
								</h1>
								{release.draft && (
									<Badge variant="yellow" pill>
										Draft
									</Badge>
								)}
								{release.release_candidate && (
									<Badge variant="orange" pill>
										RC
									</Badge>
								)}
								{release.lts && (
									<Badge variant="purple" pill>
										LTS
									</Badge>
								)}
								{release.yanked && (
									<Badge variant="red" pill>
										Yanked
									</Badge>
								)}
								{lastDeployment && (
									<Link to={`/deployments/${lastDeployment.id}`}>
										<StateBadge state={lastDeployment.state} />
									</Link>
								)}
							</div>
							<div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
								<div className="flex items-center space-x-1">
									<Calendar className="w-4 h-4" />
									<span>
										Created <RelativeTime date={release.created_at} />
									</span>
								</div>
								{distribution && (
									<div className="flex items-center space-x-2">
										<span className="font-medium">{distribution.name}</span>
										<Badge
											variant={getArchVariant(distribution.architecture)}
											pill
										>
											{distribution.architecture.toUpperCase()}
										</Badge>
									</div>
								)}
							</div>
						</div>
					</div>
					<div className="flex flex-wrap items-center gap-2 lg:flex-shrink-0 lg:gap-3">
						<Button
							variant="soft"
							tone="gray"
							to={`/devices?release_id=${release.id}`}
							icon={<Cpu className="w-4 h-4" />}
						>
							View Devices
						</Button>
						{release?.draft ? (
							<Button
								variant="solid"
								tone="green"
								loading={updateReleaseHook.isPending}
								icon={<Eye className="w-4 h-4" />}
								disabled={publishCheck.data?.allowed === false}
								title={
									publishCheck.data?.allowed === false
										? "Your role does not allow publishing this draft"
										: undefined
								}
								onClick={handlePublishRelease}
							>
								{updateReleaseHook.isPending ? "Publishing..." : "Publish"}
							</Button>
						) : release.yanked ? (
							<Button
								variant="solid"
								tone="red"
								icon={<RotateCcw className="w-4 h-4" />}
								onClick={() => setShowRollbackModal(true)}
							>
								Roll back devices
							</Button>
						) : (
							<>
								{!release?.release_candidate && (
									<Button
										variant="soft"
										tone="purple"
										loading={updateReleaseHook.isPending}
										icon={<ShieldCheck className="w-4 h-4" />}
										onClick={handleToggleLts}
									>
										{release?.lts ? "Remove LTS" : "Mark as LTS"}
									</Button>
								)}
								<Button
									variant="solid"
									tone="red"
									icon={<AlertTriangle className="w-4 h-4" />}
									onClick={() => setShowYankModal(true)}
								>
									Yank
								</Button>
								{activeDeployment ? (
									<Button
										variant="solid"
										tone="blue"
										to={`/deployments/${activeDeployment.id}`}
										icon={<Loader2 className="w-4 h-4 animate-spin" />}
									>
										View Deployment
									</Button>
								) : (
									<Button
										variant="solid"
										tone="blue"
										icon={<Rocket className="w-4 h-4" />}
										onClick={handleOpenDeployModal}
									>
										Deploy
									</Button>
								)}
							</>
						)}
					</div>
				</div>
			</Card>

			{release.yanked && (
				<div className="bg-red-50 border border-red-200 rounded-lg p-4 flex items-center gap-3">
					<AlertTriangle className="w-5 h-5 text-red-600 flex-shrink-0" />
					<div className="min-w-0 flex-1">
						<h4 className="font-medium text-red-900 text-sm">
							Yanked
							{release.yanked_at && (
								<>
									{" "}
									<RelativeTime date={release.yanked_at} />
								</>
							)}
						</h4>
						<p className="text-sm text-red-800 mt-1 break-words">
							{release.yanked_reason || "No reason was recorded."}
						</p>
					</div>
					{rollback && (
						<Button
							tone="red"
							size="sm"
							icon={<RotateCcw className="w-4 h-4" />}
							to={`/deployments/${rollback.id}`}
							className="shrink-0"
						>
							View rollback
						</Button>
					)}
				</div>
			)}

			{release.draft && publishCheck.data && (
				<Panel
					icon={ArrowUpDown}
					title={
						publishCheck.data.latest_version
							? `Changes against latest (v${publishCheck.data.latest_version})`
							: "Changes"
					}
					theme={SECTION_THEMES.gray}
					bodyClassName="px-4 py-2 space-y-2"
				>
					<PackageChanges
						changes={publishCheck.data.changes}
						emptyText="Same packages as latest so far."
					/>
					<PreflightNotice blockedReason={publishCheck.data.blocked_reason} />
					{publishError && (
						<p className="text-sm text-red-700">{publishError}</p>
					)}
				</Panel>
			)}

			{/* Deploy Confirmation Modal */}
			<Modal
				open={showDeployModal}
				onClose={() => setShowDeployModal(false)}
				title={`Deploy Release v${release.version}`}
				subtitle={
					deployStep === 2
						? `Step 2 of 2 — ${
								pilotMode === "labels"
									? "Select labels for the pilot group"
									: "Select the pilot group's devices"
							}`
						: undefined
				}
				width={deployStep === 1 ? "w-[520px]" : "w-[900px]"}
				footer={
					deployStep === 1 ? (
						<>
							<Button
								variant="soft"
								tone="gray"
								disabled={deploying}
								onClick={() => setShowDeployModal(false)}
							>
								Cancel
							</Button>
							{!release.release_candidate ? (
								<Button
									variant="solid"
									tone="blue"
									loading={deploying}
									icon={<Rocket className="w-4 h-4" />}
									disabled={deployBlocked}
									onClick={handleDeployRelease}
								>
									{deploying ? "Starting..." : "Start Deployment"}
								</Button>
							) : (
								<Button
									variant="solid"
									tone="blue"
									disabled={deployBlocked}
									onClick={() => setDeployStep(2)}
								>
									Next: Choose Pilot Group
								</Button>
							)}
						</>
					) : (
						<div className="flex justify-between w-full">
							<Button
								variant="soft"
								tone="gray"
								icon={<ArrowLeft className="w-4 h-4" />}
								disabled={deploying}
								onClick={() => setDeployStep(1)}
							>
								Back
							</Button>
							<Button
								variant="solid"
								tone="blue"
								loading={deploying}
								icon={<Rocket className="w-4 h-4" />}
								disabled={
									deploying ||
									(pilotMode === "labels" && pilotLabels.length === 0) ||
									(pilotMode === "devices" && pilotDeviceIds.size === 0)
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
						{deployCheck.data?.active && (
							<div className="bg-amber-50 border border-amber-200 rounded-lg p-3 text-sm text-amber-800">
								<p className="font-medium">
									v{deployCheck.data.active.release_version} is still deploying
								</p>
								<p className="mt-1">
									Fleet deployments run one at a time in a distribution.{" "}
									<Link
										to={`/deployments/${deployCheck.data.active.id}`}
										className="underline font-medium"
									>
										See that deployment
									</Link>
								</p>
							</div>
						)}
						{deployCheck.data && (
							<div>
								<h4 className="text-sm font-medium text-gray-900">
									What changes
								</h4>
								<PackageChanges changes={deployCheck.data.changes} />
							</div>
						)}
						{deployCheck.data && (
							<PreflightNotice
								blockedReason={deployCheck.data.blocked_reason}
							/>
						)}
						{deployError && (
							<p className="text-sm text-red-700">{deployError}</p>
						)}
						{!deployBlocked && (
							<>
								<p className="text-sm text-gray-600">
									{release.release_candidate
										? "Choose the pilot group by labels or by hand. Up to 10 of its online devices are picked as the canary; once they are healthy, you confirm and the rest of the group updates in waves. A group under 10 devices updates directly. What the fleet follows doesn't change."
										: `Up to 10 canary devices are picked automatically by health and network quality, and update first. Once they are healthy, you confirm the rollout${
												deployCheck.data
													? `: ${deployCheck.data.devices_to_move} devices in ${deployCheck.data.waves} waves, ${deployCheck.data.interval_minutes} minutes apart`
													: ""
											}.`}
									{deployCheck.data && deployCheck.data.pinned_devices > 0
										? ` ${deployCheck.data.pinned_devices} pinned devices stay where they are.`
										: ""}
								</p>

								{release.release_candidate && (
									<div className="space-y-3">
										{(
											[
												{
													mode: "labels" as const,
													title: "By Labels",
													icon: <Tag className="w-5 h-5 text-blue-500" />,
													description:
														"The pilot group is every device with any of the labels (e.g. env=staging).",
												},
												{
													mode: "devices" as const,
													title: "Select Devices",
													icon: <Cpu className="w-5 h-5 text-purple-500" />,
													description: "Hand-pick the pilot group's devices.",
												},
											] as const
										).map((option) => (
											<button
												key={option.mode}
												onClick={() => setPilotMode(option.mode)}
												className={`w-full text-left p-4 rounded-lg border-2 transition-all cursor-pointer ${
													pilotMode === option.mode
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
								)}
							</>
						)}
					</div>
				)}

				{/* Step 2: Select & Preview */}
				{deployStep === 2 && (
					<div className="-m-6 flex-1 overflow-hidden grid grid-cols-2 divide-x divide-gray-200 min-h-[400px]">
						{/* Left column: selection controls */}
						<div className="p-6 overflow-y-auto space-y-3">
							{pilotMode === "labels" && (
								<div className="space-y-3">
									<LabelAutocomplete
										onSelect={(label) =>
											setPilotLabels((prev) => [...prev, label])
										}
										existingFilters={pilotLabels}
									/>
									{pilotLabels.length > 0 && (
										<div className="flex flex-wrap gap-1">
											{pilotLabels.map((label) => (
												<span
													key={label}
													className="inline-flex items-center px-2 py-0.5 text-xs font-medium bg-blue-100 text-blue-800 rounded-full"
												>
													{label}
													<button
														onClick={() =>
															setPilotLabels((prev) =>
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
										All up-to-date devices matching these labels will be
										included in the canary.
									</p>
								</div>
							)}

							{pilotMode === "devices" && (
								<div className="space-y-2">
									<div className="relative">
										<Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
										<input
											type="text"
											placeholder="Search by serial number..."
											value={deviceSearchQuery}
											onChange={(e) => setDeviceSearchQuery(e.target.value)}
											className="w-full pl-8 pr-3 py-1.5 text-sm border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 placeholder-gray-400 bg-white"
										/>
									</div>
									<div className="max-h-[400px] overflow-y-auto border border-gray-200 rounded-md">
										{eligibleDevicesLoading ? (
											<div className="p-4 text-center text-sm text-gray-500">
												Loading devices...
											</div>
										) : eligibleDevices.length === 0 ? (
											<div className="p-4 text-center text-sm text-gray-500">
												No eligible devices found
											</div>
										) : (
											eligibleDevices.map((device: Device) => {
												const isSelected = pilotDeviceIds.has(device.id);
												return (
													<div
														key={device.id}
														onClick={() =>
															setPilotDeviceIds((prev) => {
																const next = new Map(prev);
																if (next.has(device.id)) {
																	next.delete(device.id);
																} else {
																	next.set(device.id, device);
																}
																return next;
															})
														}
														className={`flex items-center gap-3 px-3 py-2.5 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
															isSelected ? "bg-blue-50" : "hover:bg-gray-50"
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
														{renderDeviceIndicators(device)}
														{device.labels &&
															Object.keys(device.labels).length > 0 && (
																<div className="flex flex-wrap gap-1 min-w-0">
																	{Object.entries(device.labels).map(
																		([key, value]) => (
																			<code
																				key={key}
																				className="px-1 py-0.5 text-[10px] font-mono rounded border bg-gray-100 text-gray-600 border-gray-200"
																			>
																				{key}={value}
																			</code>
																		),
																	)}
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

						{/* Right column: live preview */}
						<div className="p-6 overflow-y-auto bg-gray-50 flex flex-col">
							{pilotMode === "labels" &&
								(pilotLabels.length === 0 ? (
									<>
										<h4 className="font-medium text-gray-900 text-sm mb-1">
											Pilot group
										</h4>
										<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
											Select labels to see matching devices
										</div>
									</>
								) : labelDevicesLoading ? (
									<>
										<h4 className="font-medium text-gray-900 text-sm mb-3">
											Pilot group
										</h4>
										<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
											Loading...
										</div>
									</>
								) : labelMatchDevices.length === 0 ? (
									<>
										<h4 className="font-medium text-gray-900 text-sm mb-1">
											No devices in the pilot group
										</h4>
										<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
											No matching devices
										</div>
									</>
								) : (
									<>
										<h4 className="font-medium text-gray-900 text-sm mb-3">
											{pilotGroupHeading(labelMatchDevices.length)}
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

							{pilotMode === "devices" &&
								(pilotDeviceIds.size === 0 ? (
									<>
										<h4 className="font-medium text-gray-900 text-sm mb-1">
											Pilot group
										</h4>
										<div className="flex-1 flex items-center justify-center text-sm text-gray-400">
											Select devices from the list
										</div>
									</>
								) : (
									<>
										<h4 className="font-medium text-gray-900 text-sm mb-3">
											{pilotGroupHeading(pilotDeviceIds.size)}
										</h4>
										<div className="border border-gray-200 rounded-md bg-white overflow-y-auto flex-1">
											{Array.from(pilotDeviceIds.values()).map(
												(device: Device) => (
													<div
														key={device.id}
														className="flex items-center gap-2 px-3 py-2.5 border-b border-gray-100 last:border-b-0"
													>
														<span className="text-sm font-mono text-gray-900 flex-shrink-0">
															{device.serial_number}
														</span>
														{renderDeviceIndicators(device)}
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
														<button
															onClick={() =>
																setPilotDeviceIds((prev) => {
																	const next = new Map(prev);
																	next.delete(device.id);
																	return next;
																})
															}
															className="text-gray-400 hover:text-red-500 cursor-pointer ml-auto flex-shrink-0"
														>
															<X className="w-3.5 h-3.5" />
														</button>
													</div>
												),
											)}
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
					if (!yanking) closeYankModal();
				}}
				title={`Yank Release v${release.version}`}
				width="w-[480px]"
				footer={
					<>
						<Button
							variant="soft"
							tone="gray"
							disabled={yanking}
							onClick={closeYankModal}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="red"
							loading={yanking}
							disabled={!yankReason.trim()}
							icon={<AlertTriangle className="w-4 h-4" />}
							onClick={handleYankRelease}
						>
							{yanking
								? "Yanking..."
								: rollbackAfterYank && rollbackTarget
									? "Yank & Roll Back"
									: "Yank Release"}
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
									Yanking a release will prevent devices from updating to this
									version. This action cannot be undone.
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

					<div className="space-y-2">
						<label className="flex items-start space-x-2 text-sm text-gray-700">
							<input
								type="checkbox"
								checked={rollbackAfterYank}
								onChange={(e) => setRollbackAfterYank(e.target.checked)}
								className="mt-0.5"
							/>
							<span>
								<span className="font-medium">Roll back devices now</span>
								<span className="block text-gray-500">
									Every device targeting v{release.version} is moved to the
									selected release immediately, without a canary phase.
								</span>
							</span>
						</label>
						{rollbackAfterYank && renderRollbackTargetSelect()}
					</div>
				</div>
			</Modal>

			{/* Roll Back Modal */}
			<Modal
				open={showRollbackModal}
				onClose={() => {
					if (!rollbackReleaseHook.isPending) {
						setShowRollbackModal(false);
						setRollbackTargetId(null);
					}
				}}
				title={`Roll back from v${release.version}`}
				width="w-[480px]"
				footer={
					<>
						<Button
							variant="soft"
							tone="gray"
							disabled={rollbackReleaseHook.isPending}
							onClick={() => {
								setShowRollbackModal(false);
								setRollbackTargetId(null);
							}}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="red"
							loading={rollbackReleaseHook.isPending}
							disabled={!rollbackTarget}
							icon={<RotateCcw className="w-4 h-4" />}
							onClick={handleRollback}
						>
							{rollbackReleaseHook.isPending ? "Rolling back..." : "Roll Back"}
						</Button>
					</>
				}
			>
				<div className="space-y-4">
					<p className="text-sm text-gray-700">
						Every device still targeting v{release.version} is moved to the
						selected release immediately, without a canary phase. Pinned devices
						are included.
					</p>
					{renderRollbackTargetSelect()}
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
								variant="soft"
								tone="gray"
								onClick={() => setShowReplacePackageModal(false)}
							>
								Cancel
							</Button>
							<Button
								variant="solid"
								tone="blue"
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
							<div className="font-medium text-gray-900">Current Version</div>
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
													pkg.architecture === distribution.architecture) &&
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
													onClick={() => setSelectedAvailablePackage(pkg.id)}
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
																<Badge
																	variant={getArchVariant(pkg.architecture)}
																>
																	{pkg.architecture}
																</Badge>
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
							variant="soft"
							tone="gray"
							onClick={() => setShowAddPackageModal(false)}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="blue"
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

								const filteredPackages = Array.from(packagesByName.values())
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
												onClick={() => setSelectedAvailablePackage(pkg.id)}
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
															<Badge variant={getArchVariant(pkg.architecture)}>
																{pkg.architecture}
															</Badge>
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

			{/* Base OS — only rendered when the release carries an image, so a
			    packages-only release reads exactly as it did before. */}
			{!osLoading && os && (
				<SectionCard
					icon={HardDrive}
					title="Base OS"
					theme={SECTION_THEMES.green}
					actions={
						<div className="flex items-center space-x-1">
							<Button
								variant="solid"
								tone="green"
								size="sm"
								icon={
									downloadingOs ? (
										<Loader2 className="w-4 h-4 animate-spin" />
									) : (
										<Download className="w-4 h-4" />
									)
								}
								onClick={handleDownloadOs}
								disabled={os.status !== "ready" || downloadingOs}
								title={
									os.status === "ready"
										? "Download the image"
										: "The image upload has not completed"
								}
							>
								Download
							</Button>
							{release?.draft && (
								<Button
									variant="ghost"
									tone="red"
									icon={<Trash2 className="w-4 h-4" />}
									onClick={handleDeleteOs}
									title="Remove OS image"
								/>
							)}
						</div>
					}
				>
					<ListRow>
						<div className="flex items-center space-x-3 min-w-0">
							<div className="p-2 bg-green-100 text-green-600 rounded flex-shrink-0">
								<HardDrive className="w-4 h-4" />
							</div>
							<div className="min-w-0">
								<div className="flex items-center space-x-2">
									<span className="font-medium text-gray-900 truncate">
										{os.file_name}
									</span>
									{os.status === "ready" ? (
										<Badge variant="green" pill>
											Ready
										</Badge>
									) : os.status === "pending" ? (
										<Badge variant="yellow" pill>
											Uploading
										</Badge>
									) : (
										<Badge variant="red" pill>
											Failed
										</Badge>
									)}
								</div>
								<div className="flex items-center space-x-2 text-xs text-gray-500 mt-1">
									<span>{humanBytes(os.size_bytes)}</span>
									{os.uploaded_at && (
										<>
											<span>·</span>
											<RelativeTime date={os.uploaded_at} />
										</>
									)}
								</div>
								{/* The sha256 is what the device verifies before it writes the
								    image to disk, so it is shown in full rather than elided. */}
								<p
									className="text-xs text-gray-400 font-mono mt-1 break-all"
									title="sha256"
								>
									{os.checksum}
								</p>
							</div>
						</div>
					</ListRow>
				</SectionCard>
			)}

			{/* Packages and Services Grid */}
			<div className="grid grid-cols-1 lg:grid-cols-2 gap-6 items-start">
				{/* Packages Section */}
				<SectionCard
					icon={Box}
					title="Packages"
					count={packages.length}
					theme={SECTION_THEMES.blue}
					actions={
						release?.draft ? (
							<Button
								variant="solid"
								tone="blue"
								size="sm"
								icon={<Plus className="w-4 h-4" />}
								onClick={openAddModal}
							>
								Add
							</Button>
						) : undefined
					}
				>
					{packagesLoading ? (
						<div className="p-6 text-center">
							<div className="text-gray-500 text-sm">Loading packages...</div>
						</div>
					) : packages.length === 0 ? (
						<div className="p-6 text-center">
							<Box className="w-8 h-8 text-gray-400 mx-auto mb-2" />
							<p className="text-sm text-gray-500">No packages found</p>
						</div>
					) : (
						packages.map((pkg) => {
							const latestVersion = getLatestVersionForPackage(pkg);
							const isUpgrading = upgradingPackages.has(pkg.id);
							return (
								<ListRow key={pkg.id}>
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
									{release?.draft ? (
										<div className="flex items-center space-x-1 flex-shrink-0">
											<Button
												variant="ghost"
												tone="gray"
												icon={<ChevronsUpDown className="w-4 h-4" />}
												onClick={() => openReplaceModal(pkg)}
												title="Select different version"
											/>
											<Button
												variant="ghost"
												tone="red"
												icon={<Trash2 className="w-4 h-4" />}
												onClick={() => handleDeletePackage(pkg.id)}
												title="Remove package"
											/>
										</div>
									) : null}
								</ListRow>
							);
						})
					)}
				</SectionCard>

				{/* Services Section */}
				<SectionCard
					icon={Cog}
					title="Services"
					count={services.length}
					theme={SECTION_THEMES.purple}
				>
					{servicesLoading ? (
						<div className="p-6 text-center">
							<div className="text-gray-500 text-sm">Loading services...</div>
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
						services.map((service) => {
							const packageName = getPackageNameForService(service.package_id);
							return (
								<ListRow key={service.id}>
									<div className="flex items-center space-x-3 min-w-0">
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
								</ListRow>
							);
						})
					)}
				</SectionCard>
			</div>

			{!release.draft && (
				<SectionCard
					icon={Rocket}
					title="Deployments"
					count={releaseDeployments.length}
					theme={SECTION_THEMES.blue}
				>
					{deploymentsLoading ? (
						<p className="p-6 text-center text-sm text-gray-500">
							Loading deployments...
						</p>
					) : deploymentsError ? (
						<p role="alert" className="p-6 text-center text-sm text-red-600">
							Could not load deployments.
						</p>
					) : releaseDeployments.length === 0 ? (
						<div className="p-6 text-center">
							<Rocket className="w-8 h-8 text-gray-400 mx-auto mb-2" />
							<p className="text-sm text-gray-500">
								This release has not been deployed yet
							</p>
						</div>
					) : (
						releaseDeployments.map((deployment) => (
							<ListRow
								key={deployment.id}
								to={`/deployments/${deployment.id}`}
								className="gap-4"
							>
								{/* flex-1: Firefox otherwise sizes this to almost nothing and truncates it. */}
								<div className="min-w-0 flex-1">
									<div className="flex flex-wrap items-center gap-2">
										<span className="text-xs text-gray-400">
											#{deployment.id}
										</span>
										<span className="font-medium text-gray-900">
											{deployment.kind === "rollback"
												? `Rollback from v${deployment.rolled_back_from_version ?? "?"}`
												: deployment.kind === "direct"
													? "Selected devices"
													: deployment.release_candidate
														? "Pilot group"
														: "Fleet"}
										</span>
										<StateBadge state={deployment.state} />
									</div>
									<div className="text-xs text-gray-500 mt-1 truncate">
										{deployment.created_by ?? "Unknown"} ·{" "}
										<RelativeTime date={deployment.created_at} />
										{deployment.base_version &&
											` · from v${deployment.base_version}`}
									</div>
								</div>
								<div className="w-40 flex-shrink-0 space-y-1 text-right">
									{deployment.counts.total > 0 ? (
										<>
											<ProgressBar counts={deployment.counts} />
											<div className="text-xs text-gray-500 tabular-nums">
												{deployment.counts.updated}/{deployment.counts.total}{" "}
												updated
											</div>
										</>
									) : (
										<span className="text-xs text-gray-400">Not tracked</span>
									)}
								</div>
							</ListRow>
						))
					)}
				</SectionCard>
			)}
		</PageContainer>
	);
};

export default ReleaseDetailPage;
