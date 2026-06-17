import {
	ChevronRight,
	Cpu,
	Eye,
	EyeOff,
	HardDrive,
	Layers,
	Monitor,
	Package,
} from "lucide-react";
import { useState } from "react";
import {
	BADGE_COLORS,
	Badge,
	type BadgeVariant,
	Card,
	ListRow,
	PageContainer,
	SearchInput,
} from "@/app/components/ui";
import {
	type DistributionRolloutStats,
	useGetDistributionRollouts,
	useGetDistributions,
} from "../../api-client";

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

const getProgressVariant = (progress: number): BadgeVariant => {
	if (progress === 100) return "green";
	if (progress >= 75) return "blue";
	if (progress >= 50) return "yellow";
	return "red";
};

const DistributionsPage = () => {
	const [showEmptyDistributions, setShowEmptyDistributions] = useState(false);
	const [searchTerm, setSearchTerm] = useState("");

	const { data: distributions = [], isLoading: loading } =
		useGetDistributions();
	const { data: rollouts = new Map<number, DistributionRolloutStats>() } =
		useGetDistributionRollouts({
			query: {
				select: (data) => {
					return data.reduce(
						(prev, curr) => {
							prev.set(curr.distribution_id, curr);
							return prev;
						},
						new Map() as Map<number, DistributionRolloutStats>,
					);
				},
			},
		});

	// Filter distributions based on device count and search
	const distributionsWithDevices = distributions.filter((dist) => {
		const rollout = rollouts.get(dist.id);
		return rollout && (rollout.total_devices || 0) > 0;
	});

	const distributionsWithoutDevices = distributions.filter((dist) => {
		const rollout = rollouts.get(dist.id);
		return !rollout || (rollout.total_devices || 0) === 0;
	});

	// Apply search filter
	const displayedDistributions = (
		showEmptyDistributions ? distributions : distributionsWithDevices
	).filter((dist) => {
		if (!searchTerm) return true;
		const searchLower = searchTerm.toLowerCase();
		return (
			dist.name.toLowerCase().includes(searchLower) ||
			dist.architecture.toLowerCase().includes(searchLower) ||
			dist.description?.toLowerCase().includes(searchLower)
		);
	});

	if (loading) {
		return (
			<div className="flex items-center justify-center h-32">
				<div className="text-gray-500 text-sm">Loading...</div>
			</div>
		);
	}

	return (
		<PageContainer>
			{/* Search and Distribution Count */}
			<div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
				<SearchInput
					value={searchTerm}
					onChange={setSearchTerm}
					placeholder="Search distributions..."
					className="w-full sm:w-72"
				/>

				<div className="flex items-center space-x-3">
					<span className="text-sm text-gray-500">
						{`${displayedDistributions.length} distribution${displayedDistributions.length !== 1 ? "s" : ""} shown`}
					</span>
					{distributionsWithoutDevices.length > 0 && (
						<button
							type="button"
							onClick={() => setShowEmptyDistributions(!showEmptyDistributions)}
							className="flex items-center space-x-1 text-blue-600 hover:text-blue-800 text-sm cursor-pointer"
						>
							{showEmptyDistributions ? (
								<EyeOff className="w-3 h-3" />
							) : (
								<Eye className="w-3 h-3" />
							)}
							<span>
								{showEmptyDistributions ? "Hide" : "Show"}{" "}
								{distributionsWithoutDevices.length} empty
							</span>
						</button>
					)}
				</div>
			</div>

			{/* Distributions List */}
			<Card className="overflow-hidden">
				{displayedDistributions.length === 0 ? (
					<div className="p-6 text-center">
						<Layers className="w-8 h-8 text-gray-400 mx-auto mb-2" />
						<p className="text-sm text-gray-500">No distributions found</p>
					</div>
				) : (
					<div className="divide-y divide-gray-100">
						{displayedDistributions.map((distribution) => {
							const archVariant = getArchVariant(distribution.architecture);
							const rollout = rollouts.get(distribution.id);
							const totalDevices = rollout?.total_devices || 0;
							const progress =
								totalDevices > 0
									? Math.round(
											((rollout?.updated_devices || 0) / totalDevices) * 100,
										)
									: null;
							return (
								<ListRow
									key={distribution.id}
									to={`/distributions/${distribution.id}`}
								>
									<div className="flex items-center space-x-3 min-w-0">
										<div
											className={`p-1.5 rounded-lg ${BADGE_COLORS[archVariant]}`}
										>
											{getArchIcon(distribution.architecture)}
										</div>
										<div className="min-w-0 flex-1">
											<div className="flex items-center space-x-2">
												<h4 className="text-sm font-medium text-gray-900 truncate">
													{distribution.name}
												</h4>
												<Badge
													variant={archVariant}
													pill
													className="flex-shrink-0"
												>
													{distribution.architecture.toUpperCase()}
												</Badge>
											</div>
											{distribution.description && (
												<p className="text-xs text-gray-500 truncate mt-0.5">
													{distribution.description}
												</p>
											)}
										</div>
									</div>
									<div className="flex items-center space-x-2 flex-shrink-0">
										{rollout &&
											(progress !== null ? (
												<>
													<div className="text-xs text-gray-700 font-medium tabular-nums">
														{rollout.updated_devices || 0}/{totalDevices}
													</div>
													<Badge variant={getProgressVariant(progress)}>
														{progress}%
													</Badge>
												</>
											) : (
												<div className="text-xs text-gray-500">0/0</div>
											))}
										<ChevronRight className="w-3 h-3 text-gray-400" />
									</div>
								</ListRow>
							);
						})}
					</div>
				)}
			</Card>
		</PageContainer>
	);
};

export default DistributionsPage;
