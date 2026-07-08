import {
	Card,
	ListRow,
	PageContainer,
	SearchInput,
	Toast,
	type ToastState,
} from "@teton/smith-ui";
import { Signal, Smartphone } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { useGetModemList } from "../../api-client";

const ModemSkeleton = () => (
	<div className="flex items-center justify-between px-4 py-3 animate-pulse">
		<div className="flex items-center space-x-3">
			<div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0" />
			<div className="h-4 bg-gray-300 rounded w-40" />
		</div>
		<div className="flex items-center space-x-4">
			<div className="h-4 bg-gray-200 rounded w-24" />
			<div className="h-3 bg-gray-200 rounded w-10" />
		</div>
	</div>
);

const ModemsPage = () => {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const [searchTerm, setSearchTerm] = useState("");
	const [toast, setToast] = useState<ToastState | null>(null);

	const { data: modems = [], isLoading: initialLoading } = useGetModemList();

	// Initialize search term from URL params
	useEffect(() => {
		const urlSearch = searchParams.get("search") || "";
		setSearchTerm(urlSearch);
	}, [searchParams]);

	const filteredModems =
		searchTerm === ""
			? modems
			: modems.filter(
					(modem) =>
						modem.imei.toLowerCase().includes(searchTerm.toLowerCase()) ||
						modem.network_provider
							.toLowerCase()
							.includes(searchTerm.toLowerCase()),
				);

	// Auto-hide toast after 3 seconds
	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	const formatTimeAgo = (date: string) => {
		const now = new Date();
		const past = new Date(date);
		const diff = now.getTime() - past.getTime();
		const minutes = Math.floor(diff / 60000);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days}d`;
		if (hours > 0) return `${hours}h`;
		return `${minutes}m`;
	};

	const updateSearchInUrl = (newSearchTerm: string) => {
		const params = new URLSearchParams(searchParams);
		if (newSearchTerm) {
			params.set("search", newSearchTerm);
		} else {
			params.delete("search");
		}
		navigate(`/modems?${params.toString()}`, { preventScrollReset: true });
	};

	const handleSearchChange = (value: string) => {
		setSearchTerm(value);
		updateSearchInUrl(value);
	};

	return (
		<PageContainer>
			<Toast toast={toast} onClose={() => setToast(null)} />

			{/* Search and Modem Count */}
			<div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
				<SearchInput
					value={searchTerm}
					onChange={handleSearchChange}
					placeholder="Search modems..."
					className="w-full sm:w-72"
				/>
				<span className="text-sm text-gray-500">
					{initialLoading
						? "Loading..."
						: `${filteredModems.length} modem${filteredModems.length !== 1 ? "s" : ""} shown`}
				</span>
			</div>

			{/* Modem List */}
			<Card className="overflow-hidden">
				{initialLoading ? (
					<div className="divide-y divide-gray-100">
						{Array.from({ length: 8 }, (_, i) => (
							<ModemSkeleton key={i} />
						))}
					</div>
				) : filteredModems.length === 0 ? (
					<div className="p-12 text-center text-gray-500">
						<Smartphone className="w-12 h-12 text-gray-400 mx-auto mb-4" />
						<h3 className="text-lg font-medium text-gray-900 mb-2">
							{searchTerm ? "No matching modems found" : "No modems found"}
						</h3>
						<p className="text-gray-500">
							{searchTerm
								? "Try adjusting your search terms."
								: "No modem information has been tracked yet."}
						</p>
					</div>
				) : (
					<div className="divide-y divide-gray-100">
						{filteredModems.map((modem) => (
							<ListRow key={modem.id}>
								<div className="flex items-center space-x-3 min-w-0">
									<Smartphone className="w-4 h-4 text-gray-400 flex-shrink-0" />
									<code className="text-sm font-mono text-gray-900 truncate">
										{modem.imei}
									</code>
								</div>
								<div className="flex items-center space-x-4 flex-shrink-0 text-sm">
									<div className="flex items-center space-x-1.5 text-gray-600">
										<Signal className="w-4 h-4 text-gray-400" />
										<span className="truncate">{modem.network_provider}</span>
									</div>
									<span className="text-gray-500 tabular-nums">
										{formatTimeAgo(modem.updated_at)}
									</span>
								</div>
							</ListRow>
						))}
					</div>
				)}
			</Card>
		</PageContainer>
	);
};

export default ModemsPage;
