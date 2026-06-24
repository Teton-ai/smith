import {
	Check,
	ChevronDown,
	ChevronRight,
	Cpu,
	HardDrive,
	Layers,
	Monitor,
	Package,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
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

type DistributionFilter = "active" | "archived" | "all";

const FILTER_OPTIONS: {
	value: DistributionFilter;
	label: string;
	dot: string;
	key: string;
}[] = [
	{ value: "active", label: "Active", dot: "bg-green-500", key: "1" },
	{ value: "archived", label: "Archived", dot: "bg-gray-400", key: "2" },
	{ value: "all", label: "All", dot: "bg-blue-500", key: "3" },
];

const DistributionsPage = () => {
	const [filter, setFilter] = useState<DistributionFilter>("active");
	const [searchTerm, setSearchTerm] = useState("");
	const [showFilterDropdown, setShowFilterDropdown] = useState(false);
	const filterDropdownRef = useRef<HTMLDivElement>(null);

	// Close the filter dropdown when clicking outside of it
	useEffect(() => {
		const handleClickOutside = (event: MouseEvent) => {
			if (
				filterDropdownRef.current &&
				!filterDropdownRef.current.contains(event.target as Node)
			) {
				setShowFilterDropdown(false);
			}
		};

		if (showFilterDropdown) {
			document.addEventListener("mousedown", handleClickOutside);
		}

		return () => {
			document.removeEventListener("mousedown", handleClickOutside);
		};
	}, [showFilterDropdown]);

	// Press "f" anywhere (outside a field) to toggle the filter dropdown
	useEffect(() => {
		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.key !== "f") return;
			if (event.ctrlKey || event.metaKey || event.altKey) return;
			const target = event.target as HTMLElement;
			if (
				target.tagName === "INPUT" ||
				target.tagName === "TEXTAREA" ||
				target.tagName === "SELECT" ||
				target.isContentEditable
			)
				return;
			event.preventDefault();
			setShowFilterDropdown((open) => !open);
		};
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, []);

	// While the dropdown is open: number keys pick an option, Escape closes it
	useEffect(() => {
		if (!showFilterDropdown) return;
		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setShowFilterDropdown(false);
				return;
			}
			const option = FILTER_OPTIONS.find((o) => o.key === event.key);
			if (option) {
				event.preventDefault();
				setFilter(option.value);
				setShowFilterDropdown(false);
			}
		};
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [showFilterDropdown]);

	// Fetch archived distributions too so the filter can switch between them
	// without refetching.
	const { data: distributions = [], isLoading: loading } = useGetDistributions({
		include_archived: true,
	});
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

	// Apply archived and search filters
	const displayedDistributions = distributions
		.filter((dist) => {
			if (filter === "all") return true;
			if (filter === "archived") return dist.archived;
			return !dist.archived;
		})
		.filter((dist) => {
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
			{/* Search, Filter and Distribution Count */}
			<div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
				<div className="flex items-center gap-3">
					<SearchInput
						value={searchTerm}
						onChange={setSearchTerm}
						placeholder="Search distributions..."
						className="w-full sm:w-72"
						slashToFocus
					/>

					<div className="relative" ref={filterDropdownRef}>
						<button
							type="button"
							onClick={() => setShowFilterDropdown(!showFilterDropdown)}
							className={`flex items-center space-x-2 px-3 py-2 text-sm font-medium rounded-lg shadow-sm transition-all cursor-pointer border ${
								showFilterDropdown
									? "bg-blue-50 text-blue-700 border-blue-300"
									: "bg-white text-gray-700 border-gray-200 hover:bg-gray-50 hover:border-gray-300"
							}`}
						>
							<span
								className={`w-2 h-2 rounded-full ${
									FILTER_OPTIONS.find((o) => o.value === filter)?.dot
								}`}
							/>
							<span>
								{FILTER_OPTIONS.find((o) => o.value === filter)?.label}
							</span>
							{!showFilterDropdown && (
								<kbd className="text-gray-400 text-xs border border-gray-200 rounded px-1 py-0.5 font-mono leading-none pointer-events-none">
									f
								</kbd>
							)}
							<ChevronDown
								className={`w-4 h-4 text-gray-400 transition-transform ${showFilterDropdown ? "rotate-180" : ""}`}
							/>
						</button>

						{showFilterDropdown && (
							<div className="absolute top-full left-0 mt-1.5 w-44 bg-white border border-gray-200/80 rounded-lg shadow-lg z-50 p-1">
								{FILTER_OPTIONS.map((option) => {
									const selected = filter === option.value;
									return (
										<button
											key={option.value}
											type="button"
											onClick={() => {
												setFilter(option.value);
												setShowFilterDropdown(false);
											}}
											className={`w-full px-2.5 py-2 text-left text-sm rounded-md flex items-center justify-between cursor-pointer transition-colors ${
												selected
													? "bg-blue-50 text-blue-700 font-medium"
													: "text-gray-700 hover:bg-gray-50"
											}`}
										>
											<span className="flex items-center space-x-2">
												<span
													className={`w-2 h-2 rounded-full ${option.dot}`}
												/>
												<span>{option.label}</span>
											</span>
											<span className="flex items-center space-x-2">
												{selected && (
													<Check className="w-4 h-4 text-blue-600" />
												)}
												<kbd className="text-gray-400 text-xs border border-gray-200 rounded px-1 py-0.5 font-mono leading-none">
													{option.key}
												</kbd>
											</span>
										</button>
									);
								})}
							</div>
						)}
					</div>
				</div>

				<span className="text-sm text-gray-500">
					{`${displayedDistributions.length} distribution${displayedDistributions.length !== 1 ? "s" : ""} shown`}
				</span>
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
