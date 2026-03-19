"use client";


import {
	ArrowLeft,
	ArrowLeftRight,
	Calendar,
	ChevronRight,
	Cpu,
	HardDrive,
	Minus,
	Monitor,
	Package,
	Plus,
	Tag,
	User,
	X,
} from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import React, { useCallback, useMemo, useState } from "react";
import {
	type Package as PackageType,
	useCreateDistributionRelease,
	useGetDistributionById,
	useGetDistributionLatestRelease,
	useGetDistributionReleasePackages,
	useGetDistributionReleases,
} from "@/app/api-client";
import { Button } from "@/app/components/button";
import { Modal } from "@/app/components/modal";
import { SidePanel } from "@/app/components/side-panel";

interface ReleaseRowProps {
	release: {
		id: number;
		version: string;
		draft: boolean;
		release_candidate: boolean;
		yanked: boolean;
		created_at: string;
		user_email: string | null;
		user_id: number | null;
	};
	compareMode: boolean;
	isSelected: boolean;
	selectionLabel: string | null;
	isDeployed: boolean;
	onSelect: (id: number) => void;
}

const ReleaseRow = React.memo(function ReleaseRow({
	release,
	compareMode,
	isSelected,
	selectionLabel,
	isDeployed,
	onSelect,
}: ReleaseRowProps) {
	const row = (
		<div className="flex items-center justify-between">
			<div className="flex items-center space-x-3">
				{compareMode ? (
					<div
						className={`w-8 h-8 rounded flex items-center justify-center text-xs font-bold ${
							isSelected
								? "bg-blue-500 text-white"
								: "bg-gray-100 text-gray-400 border border-gray-300 border-dashed"
						}`}
					>
						{selectionLabel ?? ""}
					</div>
				) : (
					<div className="p-2 bg-gray-100 text-gray-600 rounded">
						<Tag className="w-4 h-4" />
					</div>
				)}
				<div>
					<div className="flex items-center space-x-2">
						<h4 className="font-medium text-gray-900">{release.version}</h4>
						{isDeployed && (
							<span className="px-2 py-1 text-xs font-medium bg-green-100 text-green-800 rounded-full">
								Deployed
							</span>
						)}
						{release.draft && (
							<span className="px-2 py-1 text-xs font-medium bg-yellow-100 text-yellow-800 rounded-full">
								Draft
							</span>
						)}
						{release.release_candidate && (
							<span className="px-2 py-1 text-xs font-medium bg-orange-100 text-orange-700 rounded-full">
								RC
							</span>
						)}
						{release.yanked && (
							<span className="px-2 py-1 text-xs font-medium bg-red-100 text-red-800 rounded-full">
								Yanked
							</span>
						)}
					</div>
					<div className="flex items-center space-x-3 mt-1 text-xs text-gray-500">
						<div className="flex items-center space-x-1">
							<Calendar className="w-3 h-3" />
							<span>{moment(release.created_at).fromNow()}</span>
						</div>
						<div className="flex items-center space-x-1">
							<User className="w-3 h-3" />
							<span>
								{release.user_email ||
									(release.user_id ? `User #${release.user_id}` : "Unknown")}
							</span>
						</div>
					</div>
				</div>
			</div>
			{!compareMode && (
				<div className="flex items-center space-x-4">
					<ChevronRight className="w-4 h-4 text-gray-400" />
				</div>
			)}
		</div>
	);

	if (compareMode) {
		return (
			<button
				type="button"
				className={`block w-full text-left p-4 cursor-pointer transition-colors ${
					isSelected ? "bg-blue-50 hover:bg-blue-100" : "hover:bg-gray-50"
				}`}
				onClick={() => onSelect(release.id)}
			>
				{row}
			</button>
		);
	}

	return (
		<Link
			className="block p-4 hover:bg-gray-50 cursor-pointer transition-colors"
			href={`/releases/${release.id}`}
		>
			{row}
		</Link>
	);
});

const DistributionDetailPage = () => {
	const router = useRouter();
	const params = useParams();
	const distributionId = parseInt(params.id as string);
	const [showCreateModal, setShowCreateModal] = useState(false);
	const [selectedVersionOption, setSelectedVersionOption] =
		useState<string>("");
	const [customVersion, setCustomVersion] = useState("");
	const [isReleaseCandidate, setIsReleaseCandidate] = useState(false);
	const [creatingDraft, setCreatingDraft] = useState(false);
	const [compareMode, setCompareMode] = useState(false);
	const [compareSelection, setCompareSelection] = useState<number[]>([]);

	const { data: distribution, isLoading: loading } =
		useGetDistributionById(distributionId);

	const { data: releases = [], isLoading: releasesLoading } =
		useGetDistributionReleases(distributionId);

	const { data: deployedRelease } =
		useGetDistributionLatestRelease(distributionId);

	// Use the most recent non-yanked release as the base for new drafts
	const baseRelease = releases.find((r) => !r.yanked) || releases[0];
	const getDistributionReleasePackages = useGetDistributionReleasePackages(
		baseRelease?.id as number,
		{ query: { enabled: baseRelease?.id != null } },
	);

	const createDistributionReleaseHook = useCreateDistributionRelease();

	const baseReleaseId = compareSelection[0] ?? null;
	const targetReleaseId = compareSelection[1] ?? null;

	const { data: baseComparePackages, isLoading: basePackagesLoading } =
		useGetDistributionReleasePackages(baseReleaseId as number, {
			query: { enabled: baseReleaseId != null },
		});
	const { data: targetComparePackages, isLoading: targetPackagesLoading } =
		useGetDistributionReleasePackages(targetReleaseId as number, {
			query: { enabled: targetReleaseId != null },
		});

	const compareLoading = basePackagesLoading || targetPackagesLoading;

	const diff = useMemo(() => {
		if (!baseComparePackages || !targetComparePackages) return null;

		const baseByName = new Map<string, PackageType>();
		for (const pkg of baseComparePackages) {
			baseByName.set(pkg.name, pkg);
		}

		const targetByName = new Map<string, PackageType>();
		for (const pkg of targetComparePackages) {
			targetByName.set(pkg.name, pkg);
		}

		const added: PackageType[] = [];
		const removed: PackageType[] = [];
		const changed: { name: string; oldVersion: string; newVersion: string }[] =
			[];
		const unchanged: PackageType[] = [];

		for (const [name, pkg] of targetByName) {
			const basePkg = baseByName.get(name);
			if (!basePkg) {
				added.push(pkg);
			} else if (basePkg.version !== pkg.version) {
				changed.push({
					name,
					oldVersion: basePkg.version,
					newVersion: pkg.version,
				});
			} else {
				unchanged.push(pkg);
			}
		}

		for (const [name, pkg] of baseByName) {
			if (!targetByName.has(name)) {
				removed.push(pkg);
			}
		}

		return { added, removed, changed, unchanged };
	}, [baseComparePackages, targetComparePackages]);

	const toggleCompareSelection = useCallback((releaseId: number) => {
		setCompareSelection((prev) => {
			if (prev.includes(releaseId)) {
				return prev.filter((id) => id !== releaseId);
			}
			if (prev.length >= 2) {
				return [prev[1], releaseId];
			}
			return [...prev, releaseId];
		});
	}, []);

	const getArchIcon = (architecture: string) => {
		switch (architecture.toLowerCase()) {
			case "x86_64":
			case "amd64":
				return <Monitor className="w-5 h-5" />;
			case "arm64":
			case "aarch64":
				return <Cpu className="w-5 h-5" />;
			case "armv7":
			case "arm":
				return <HardDrive className="w-5 h-5" />;
			default:
				return <Package className="w-5 h-5" />;
		}
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

	// Get the latest non-yanked release to use as base (includes drafts)
	const getLatestRelease = () => {
		return releases.find((release) => !release.yanked) || releases[0];
	};

	// Parse version and generate options
	const getVersionOptions = () => {
		const baseRelease = getLatestRelease();
		if (!baseRelease) return [];

		const version = baseRelease.version;
		// Try to parse semantic version (e.g., "1.2.3")
		const match = version.match(/^(\d+)\.(\d+)\.(\d+)/);

		if (match) {
			const [, major, minor, patch] = match;
			return [
				{
					type: "PATCH",
					version: `${major}.${minor}.${parseInt(patch) + 1}`,
					description: "Bug fixes and small changes",
				},
				{
					type: "MINOR",
					version: `${major}.${parseInt(minor) + 1}.0`,
					description: "New features, backwards compatible",
				},
				{
					type: "MAJOR",
					version: `${parseInt(major) + 1}.0.0`,
					description: "Significant new features, may include breaking changes",
				},
			];
		}

		// Fallback for non-semantic versions
		return [
			{
				type: "NEW",
				version: `${version}.1`,
				description: "New version",
			},
		];
	};

	const isValidSemver = (v: string) =>
		/^\d+\.\d+\.\d+(-[a-zA-Z0-9]+(\.[a-zA-Z0-9]+)*)?$/.test(v);

	const resolvedVersion =
		selectedVersionOption === "custom"
			? customVersion.trim()
			: selectedVersionOption;

	const canCreate =
		resolvedVersion &&
		(selectedVersionOption !== "custom" || isValidSemver(resolvedVersion));

	const handleCreateDraft = async () => {
		if (
			creatingDraft ||
			getDistributionReleasePackages.data == null ||
			!canCreate
		)
			return;

		setCreatingDraft(true);
		try {
			const finalVersion = isReleaseCandidate
				? `${resolvedVersion}-rc`
				: resolvedVersion;

			const newRelease = await createDistributionReleaseHook.mutateAsync({
				distributionId,
				data: {
					packages: getDistributionReleasePackages.data.map((p) => p.id),
					version: finalVersion,
					release_candidate: isReleaseCandidate,
				},
			});

			if (newRelease) {
				router.push(`/releases/${newRelease}`);
			}
		} catch (error: any) {
			console.error("Failed to create draft release:", error);
			alert(
				`Failed to create draft release: ${error?.message || "Unknown error"}`,
			);
		} finally {
			setCreatingDraft(false);
			setShowCreateModal(false);
			setSelectedVersionOption("");
			setCustomVersion("");
			setIsReleaseCandidate(false);
		}
	};

	const openCreateModal = () => {
		const latestRelease = getLatestRelease();
		if (latestRelease) {
			const options = getVersionOptions();
			if (options.length > 0) {
				setSelectedVersionOption(options[0].version); // Default to first option (PATCH)
			}
			setIsReleaseCandidate(false);
			setCustomVersion("");
			setShowCreateModal(true);
		}
	};

	if (loading || !distribution) {
		return (
			<div className="flex items-center justify-center h-32">
				<div className="text-gray-500 text-sm">Loading...</div>
			</div>
		);
	}

	return (
		<div className="flex-1 overflow-y-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
			{/* Header with Back Button */}
			<div className="flex items-center space-x-4">
				<Link
					href="/distributions"
					className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors cursor-pointer"
				>
					<ArrowLeft className="w-4 h-4" />
					<span className="text-sm font-medium">Back to Distributions</span>
				</Link>
			</div>

			{/* Distribution Header */}
			<div className="bg-white rounded-lg border border-gray-200 p-4">
				<div className="flex items-center space-x-3">
					<div
						className={`p-2 rounded ${getArchColor(distribution.architecture)}`}
					>
						{getArchIcon(distribution.architecture)}
					</div>
					<div className="flex-1">
						<div className="flex items-center space-x-3">
							<h1 className="text-xl font-bold text-gray-900">
								{distribution.name}
							</h1>
							<span
								className={`px-2 py-1 text-xs font-medium rounded-full ${getArchColor(distribution.architecture)}`}
							>
								{distribution.architecture.toUpperCase()}
							</span>
						</div>
						{distribution.description && (
							<p className="text-sm text-gray-600">
								{distribution.description}
							</p>
						)}
					</div>
				</div>
			</div>

			{/* Create Release Modal */}
			<Modal
				open={showCreateModal}
				onClose={() => {
					setShowCreateModal(false);
					setSelectedVersionOption("");
					setCustomVersion("");
					setIsReleaseCandidate(false);
				}}
				title="Draft New Release"
				subtitle={`Based on ${getLatestRelease()?.version} by ${getLatestRelease()?.user_email || (getLatestRelease()?.user_id ? `User #${getLatestRelease()?.user_id}` : "Unknown")}${getLatestRelease()?.draft ? " (draft)" : ""}`}
				width="w-[480px]"
				footer={
					<>
						<Button
							variant="secondary"
							disabled={creatingDraft}
							onClick={() => {
								setShowCreateModal(false);
								setSelectedVersionOption("");
								setCustomVersion("");
								setIsReleaseCandidate(false);
							}}
						>
							Cancel
						</Button>
						<Button
							variant="success"
							loading={creatingDraft}
							disabled={!canCreate}
							onClick={handleCreateDraft}
						>
							{creatingDraft ? "Creating..." : "Create Draft"}
						</Button>
					</>
				}
			>
				<div className="space-y-3 mb-6">
					{selectedVersionOption === "custom" ? (
						<div
							key="custom"
							className="p-3 border border-green-500 bg-green-50 rounded-lg shadow-sm animate-fade-slide-in"
						>
							<div className="flex items-center justify-between mb-2">
								<span className="text-sm font-medium text-gray-900">
									Custom Version
								</span>
								<button
									type="button"
									onClick={() => {
										setCustomVersion("");
										const options = getVersionOptions();
										setSelectedVersionOption(
											options.length > 0 ? options[0].version : "",
										);
									}}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
								>
									<X className="w-4 h-4" />
								</button>
							</div>
							<input
								type="text"
								value={customVersion}
								onChange={(e) => setCustomVersion(e.target.value)}
								placeholder="0.0.0"
								autoFocus
								className={`w-full px-3 py-2 border rounded-lg text-sm font-mono text-gray-900 placeholder-gray-400 bg-white focus:outline-none focus:ring-2 ${
									customVersion.trim() && !isValidSemver(customVersion.trim())
										? "border-red-300 focus:ring-red-500"
										: "border-gray-300 focus:ring-green-500"
								}`}
							/>
							{customVersion.trim() && !isValidSemver(customVersion.trim()) && (
								<p className="text-xs text-red-600 mt-1.5">
									Must be a valid semver version (e.g. 1.2.3, 1.0.0-alpha.1)
								</p>
							)}
							<p className="text-xs text-gray-500 mt-1.5">
								Enter a valid semver version (e.g. 2.0.0-beta.1)
							</p>
						</div>
					) : (
						<div key="options" className="space-y-3 animate-fade-slide-in">
							{getVersionOptions().map((option, i) => (
								<label
									key={option.version}
									style={{
										animationDelay: `${i * 50}ms`,
										animationFillMode: "both",
									}}
									className={`flex items-center p-3 border rounded-lg cursor-pointer transition-all duration-200 animate-fade-slide-in ${
										selectedVersionOption === option.version
											? "border-green-500 bg-green-50 shadow-sm"
											: "border-gray-200 hover:border-gray-300 hover:bg-gray-50"
									}`}
								>
									<input
										type="radio"
										name="version"
										value={option.version}
										checked={selectedVersionOption === option.version}
										onChange={(e) => setSelectedVersionOption(e.target.value)}
										className="w-4 h-4 text-green-600 border-gray-300 focus:ring-green-500"
									/>
									<div className="ml-3 flex-1">
										<div className="flex items-center space-x-3">
											<span className="font-mono text-sm font-medium text-gray-900">
												{option.version}
											</span>
											<span className="px-2 py-1 text-xs font-medium bg-gray-100 text-gray-700 rounded">
												{option.type}
											</span>
										</div>
										<p className="text-xs text-gray-600 mt-1">
											{option.description}
										</p>
									</div>
								</label>
							))}

							<button
								type="button"
								onClick={() => setSelectedVersionOption("custom")}
								style={{
									animationDelay: `${getVersionOptions().length * 50}ms`,
									animationFillMode: "both",
								}}
								className="w-full flex items-center p-3 border border-dashed border-gray-300 rounded-lg text-sm text-gray-500 hover:border-gray-400 hover:text-gray-700 hover:bg-gray-50 cursor-pointer transition-all duration-200 animate-fade-slide-in"
							>
								<span className="ml-1">Use custom version...</span>
							</button>
						</div>
					)}
				</div>

				<label className="flex items-center space-x-3 p-3 border border-gray-200 rounded-lg hover:bg-gray-50 cursor-pointer transition-colors">
					<input
						type="checkbox"
						checked={isReleaseCandidate}
						onChange={(e) => setIsReleaseCandidate(e.target.checked)}
						className="w-4 h-4 text-orange-600 border-gray-300 rounded focus:ring-orange-500"
					/>
					<div className="flex-1">
						<div className="flex items-center space-x-2">
							<span className="text-sm font-medium text-gray-900">
								Release Candidate
							</span>
							<span className="px-2 py-1 text-xs font-medium bg-orange-100 text-orange-700 rounded">
								RC
							</span>
						</div>
						<p className="text-xs text-gray-600 mt-1">
							Adds "-rc" suffix:{" "}
							{resolvedVersion
								? `${resolvedVersion}${isReleaseCandidate ? "-rc" : ""}`
								: "x.x.x"}
						</p>
					</div>
				</label>
			</Modal>

			{/* Releases Section */}
			<div className="space-y-4">
				<div className="flex items-center justify-between">
					<div className="flex items-center space-x-2">
						<Tag className="w-5 h-5 text-gray-600" />
						<h2 className="text-lg font-semibold text-gray-900">Releases</h2>
						<span className="text-sm text-gray-500">({releases.length})</span>
					</div>
					<div className="flex items-center space-x-2">
						{releases.length >= 2 && (
							<Button
								variant={compareMode ? "primary" : "secondary"}
								icon={
									compareMode ? (
										<X className="w-4 h-4" />
									) : (
										<ArrowLeftRight className="w-4 h-4" />
									)
								}
								onClick={() => {
									setCompareMode(!compareMode);
									setCompareSelection([]);
								}}
							>
								{compareMode ? "Exit Compare" : "Compare"}
							</Button>
						)}
						{releases.length > 0 && (
							<Button
								variant="success"
								icon={<Plus className="w-4 h-4" />}
								onClick={openCreateModal}
							>
								Draft New Release
							</Button>
						)}
					</div>
				</div>

				<div className="bg-white rounded border border-gray-200 overflow-hidden">
					{releasesLoading ? (
						<div className="p-6 text-center">
							<div className="text-gray-500 text-sm">Loading releases...</div>
						</div>
					) : releases.length === 0 ? (
						<div className="p-6 text-center">
							<Tag className="w-8 h-8 text-gray-400 mx-auto mb-2" />
							<p className="text-sm text-gray-500">
								No releases found for this distribution
							</p>
						</div>
					) : (
						<div className="divide-y divide-gray-200">
							{compareMode && (
								<div className="px-4 py-2 bg-blue-50 border-b border-blue-100 text-xs text-blue-700">
									{compareSelection.length === 0
										? "Select two releases to compare"
										: compareSelection.length === 1
											? "Select one more release"
											: "Showing diff below — click a release to change selection"}
								</div>
							)}
							{releases.map((release) => {
								const selectionIndex = compareSelection.indexOf(release.id);
								return (
									<ReleaseRow
										key={release.id}
										release={release}
										compareMode={compareMode}
										isSelected={selectionIndex !== -1}
										selectionLabel={
											selectionIndex === 0
												? "A"
												: selectionIndex === 1
													? "B"
													: null
										}
										isDeployed={deployedRelease?.id === release.id}
										onSelect={toggleCompareSelection}
									/>
								);
							})}
						</div>
					)}
				</div>
			</div>

			{/* Compare side panel */}
			<SidePanel
				open={compareMode && compareSelection.length === 2}
				onClose={() => {
					setCompareMode(false);
					setCompareSelection([]);
				}}
				title="Compare Releases"
				subtitle={
					baseReleaseId && targetReleaseId
						? `${releases.find((r) => r.id === baseReleaseId)?.version} → ${releases.find((r) => r.id === targetReleaseId)?.version}`
						: undefined
				}
				headerRight={
					diff ? (
						<div className="flex items-center space-x-1">
							{diff.added.length > 0 && (
								<span className="px-1.5 py-0.5 text-xs font-medium bg-green-100 text-green-700 rounded">
									+{diff.added.length}
								</span>
							)}
							{diff.removed.length > 0 && (
								<span className="px-1.5 py-0.5 text-xs font-medium bg-red-100 text-red-700 rounded">
									-{diff.removed.length}
								</span>
							)}
							{diff.changed.length > 0 && (
								<span className="px-1.5 py-0.5 text-xs font-medium bg-amber-100 text-amber-700 rounded">
									~{diff.changed.length}
								</span>
							)}
						</div>
					) : undefined
				}
			>
				{compareLoading ? (
					<div className="flex items-center justify-center py-12">
						<div className="text-gray-500 text-sm">Loading packages...</div>
					</div>
				) : diff ? (
					(() => {
						let idx = 0;
						const allEmpty =
							diff.added.length === 0 &&
							diff.removed.length === 0 &&
							diff.changed.length === 0;

						return (
							<div className="space-y-1.5">
								{allEmpty && (
									<p className="text-sm text-gray-500 py-4 text-center animate-fade-slide-in">
										No differences between these releases.
									</p>
								)}

								{diff.added.length > 0 && (
									<h4
										style={{
											animationDelay: `${idx++ * 30}ms`,
											animationFillMode: "both",
										}}
										className="text-xs font-medium text-gray-400 uppercase tracking-wide pt-1 animate-fade-slide-in"
									>
										Added
									</h4>
								)}
								{diff.added.map((pkg) => (
									<div
										key={`add-${pkg.name}`}
										style={{
											animationDelay: `${idx++ * 30}ms`,
											animationFillMode: "both",
										}}
										className="flex items-center space-x-2 px-3 py-2 bg-green-50 border-l-2 border-green-500 rounded-r animate-fade-slide-in"
									>
										<Plus className="w-3.5 h-3.5 text-green-600 flex-shrink-0" />
										<span className="text-sm font-medium text-gray-900">
											{pkg.name}
										</span>
										<span className="text-xs font-mono text-gray-500">
											{pkg.version}
										</span>
									</div>
								))}

								{diff.removed.length > 0 && (
									<h4
										style={{
											animationDelay: `${idx++ * 30}ms`,
											animationFillMode: "both",
										}}
										className="text-xs font-medium text-gray-400 uppercase tracking-wide pt-1 animate-fade-slide-in"
									>
										Removed
									</h4>
								)}
								{diff.removed.map((pkg) => (
									<div
										key={`rm-${pkg.name}`}
										style={{
											animationDelay: `${idx++ * 30}ms`,
											animationFillMode: "both",
										}}
										className="flex items-center space-x-2 px-3 py-2 bg-red-50 border-l-2 border-red-500 rounded-r animate-fade-slide-in"
									>
										<Minus className="w-3.5 h-3.5 text-red-600 flex-shrink-0" />
										<span className="text-sm font-medium text-gray-900">
											{pkg.name}
										</span>
										<span className="text-xs font-mono text-gray-500">
											{pkg.version}
										</span>
									</div>
								))}

								{diff.changed.length > 0 && (
									<h4
										style={{
											animationDelay: `${idx++ * 30}ms`,
											animationFillMode: "both",
										}}
										className="text-xs font-medium text-gray-400 uppercase tracking-wide pt-1 animate-fade-slide-in"
									>
										Changed
									</h4>
								)}
								{diff.changed.map((pkg) => (
									<div
										key={`chg-${pkg.name}`}
										style={{
											animationDelay: `${idx++ * 30}ms`,
											animationFillMode: "both",
										}}
										className="flex items-center space-x-2 px-3 py-2 bg-amber-50 border-l-2 border-amber-500 rounded-r animate-fade-slide-in"
									>
										<span className="text-amber-600 flex-shrink-0 text-sm font-bold">
											~
										</span>
										<span className="text-sm font-medium text-gray-900">
											{pkg.name}
										</span>
										<span className="text-xs font-mono text-gray-500 line-through">
											{pkg.oldVersion}
										</span>
										<span className="text-xs text-gray-400">→</span>
										<span className="text-xs font-mono text-gray-900">
											{pkg.newVersion}
										</span>
									</div>
								))}

								{diff.unchanged.length > 0 && (
									<>
										<h4
											style={{
												animationDelay: `${idx++ * 30}ms`,
												animationFillMode: "both",
											}}
											className="text-xs font-medium text-gray-400 uppercase tracking-wide pt-1 animate-fade-slide-in"
										>
											Unchanged
										</h4>
										{diff.unchanged.map((pkg) => (
											<div
												key={`unch-${pkg.name}`}
												style={{
													animationDelay: `${idx++ * 30}ms`,
													animationFillMode: "both",
												}}
												className="flex items-center space-x-2 px-3 py-2 bg-gray-50 border-l-2 border-gray-300 rounded-r animate-fade-slide-in"
											>
												<span className="text-sm text-gray-600">
													{pkg.name}
												</span>
												<span className="text-xs font-mono text-gray-400">
													{pkg.version}
												</span>
											</div>
										))}
									</>
								)}
							</div>
						);
					})()
				) : null}
			</SidePanel>
		</div>
	);
};

export default DistributionDetailPage;
