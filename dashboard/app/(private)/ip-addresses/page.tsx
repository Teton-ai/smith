import { useQueryClient } from "@tanstack/react-query";
import { Check, Edit2, Globe, Shield, Wifi, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import {
	Badge,
	Card,
	CountryFlag,
	ListRow,
	PageContainer,
	SearchInput,
	Toast,
	type ToastState,
} from "@/app/components/ui";
import {
	type IpAddressInfo,
	useGetIpAddresses,
	useUpdateIpAddress,
} from "../../api-client";

const IpAddressSkeleton = () => (
	<div className="flex items-center justify-between px-4 py-3 animate-pulse">
		<div className="flex items-center space-x-3">
			<div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0" />
			<div className="h-4 bg-gray-300 rounded w-32" />
		</div>
		<div className="flex items-center space-x-4">
			<div className="h-4 bg-gray-200 rounded w-28" />
			<div className="h-3 bg-gray-200 rounded w-10" />
		</div>
	</div>
);

const IpAddressesPage = () => {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const queryClient = useQueryClient();
	const [searchTerm, setSearchTerm] = useState("");
	const [editingId, setEditingId] = useState<number | null>(null);
	const [editingName, setEditingName] = useState("");
	const [saving, setSaving] = useState(false);
	const [toast, setToast] = useState<ToastState | null>(null);

	const {
		data: ipAddresses = [],
		isLoading: initialLoading,
		queryKey: ipAddressesQueryKey,
	} = useGetIpAddresses({ query: { select: (data) => data.ip_addresses } });

	// Initialize search term from URL params
	useEffect(() => {
		const urlSearch = searchParams.get("search") || "";
		setSearchTerm(urlSearch);
	}, [searchParams]);

	const filteredIpAddresses =
		searchTerm === ""
			? ipAddresses
			: ipAddresses.filter(
					(ip) =>
						ip.ip_address.toLowerCase().includes(searchTerm.toLowerCase()) ||
						ip.name?.toLowerCase().includes(searchTerm.toLowerCase()) ||
						ip.country?.toLowerCase().includes(searchTerm.toLowerCase()) ||
						ip.city?.toLowerCase().includes(searchTerm.toLowerCase()) ||
						ip.isp?.toLowerCase().includes(searchTerm.toLowerCase()),
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

	const getLocationString = (ip: IpAddressInfo) => {
		const parts = [];
		if (ip.city) parts.push(ip.city);
		if (ip.region && ip.region !== ip.city) parts.push(ip.region);
		if (ip.country) parts.push(ip.country);
		return parts.join(", ") || "Unknown Location";
	};

	const updateSearchInUrl = (newSearchTerm: string) => {
		const params = new URLSearchParams(searchParams);
		if (newSearchTerm) {
			params.set("search", newSearchTerm);
		} else {
			params.delete("search");
		}
		navigate(`/ip-addresses?${params.toString()}`, {
			preventScrollReset: true,
		});
	};

	const handleSearchChange = (value: string) => {
		setSearchTerm(value);
		updateSearchInUrl(value);
	};

	const startEditing = (ip: IpAddressInfo) => {
		setEditingId(ip.id);
		setEditingName(ip.name || "");
	};

	const cancelEditing = () => {
		setEditingId(null);
		setEditingName("");
	};

	const updateIpAddressHook = useUpdateIpAddress();
	const saveEdit = async () => {
		if (editingId == null || saving) return;

		setSaving(true);
		try {
			const updatedIp = await updateIpAddressHook.mutateAsync({
				ipAddressId: editingId,
				data: { name: editingName.trim() },
			});

			if (updatedIp) {
				queryClient.invalidateQueries({ queryKey: ipAddressesQueryKey });

				// Show success toast
				const name = editingName.trim();
				setToast({
					message: name
						? `IP address renamed to "${name}"`
						: "IP address name cleared",
					type: "success",
				});
			}

			setEditingId(null);
			setEditingName("");
		} catch (error: any) {
			console.error("Failed to update IP address name:", error);
			setToast({
				message: `Failed to update: ${error?.message || "Unknown error"}`,
				type: "error",
			});
		} finally {
			setSaving(false);
		}
	};

	return (
		<PageContainer>
			<Toast toast={toast} onClose={() => setToast(null)} />

			{/* Search and IP Count */}
			<div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
				<SearchInput
					value={searchTerm}
					onChange={handleSearchChange}
					placeholder="Search IP addresses..."
					className="w-full sm:w-72"
				/>
				<span className="text-sm text-gray-500">
					{initialLoading
						? "Loading..."
						: `${filteredIpAddresses.length} IP address${filteredIpAddresses.length !== 1 ? "es" : ""} shown`}
				</span>
			</div>

			{/* IP Address List */}
			<Card className="overflow-hidden">
				{initialLoading ? (
					<div className="divide-y divide-gray-100">
						{Array.from({ length: 8 }, (_, i) => (
							<IpAddressSkeleton key={i} />
						))}
					</div>
				) : filteredIpAddresses.length === 0 ? (
					<div className="p-12 text-center text-gray-500">
						<Globe className="w-12 h-12 text-gray-400 mx-auto mb-4" />
						<h3 className="text-lg font-medium text-gray-900 mb-2">
							{searchTerm
								? "No matching IP addresses found"
								: "No IP addresses found"}
						</h3>
						<p className="text-gray-500">
							{searchTerm
								? "Try adjusting your search terms."
								: "No IP address information has been tracked yet."}
						</p>
					</div>
				) : (
					<div className="divide-y divide-gray-100">
						{filteredIpAddresses.map((ipInfo) => {
							const isEditing = editingId === ipInfo.id;
							return (
								<ListRow key={ipInfo.id}>
									{/* Left: identity */}
									<div className="flex items-center space-x-3 min-w-0">
										<Globe className="w-4 h-4 text-gray-400 flex-shrink-0" />
										{isEditing ? (
											<div className="flex items-center space-x-2">
												<input
													type="text"
													value={editingName}
													onChange={(e) => setEditingName(e.target.value)}
													placeholder={ipInfo.ip_address}
													disabled={saving}
													className={`text-sm font-medium text-gray-900 bg-white border border-gray-300 rounded px-2 py-1 min-w-0 flex-1 ${
														saving ? "opacity-50 cursor-not-allowed" : ""
													}`}
													onKeyDown={(e) => {
														if (e.key === "Enter" && !saving) saveEdit();
														if (e.key === "Escape" && !saving) cancelEditing();
													}}
													autoFocus
												/>
												<button
													type="button"
													onClick={saveEdit}
													disabled={saving}
													className={`p-1 transition-colors ${
														saving
															? "text-gray-400 cursor-not-allowed"
															: "text-green-600 hover:text-green-800 cursor-pointer"
													}`}
												>
													{saving ? (
														<div className="w-4 h-4 border-2 border-green-600 border-t-transparent rounded-full animate-spin" />
													) : (
														<Check className="w-4 h-4" />
													)}
												</button>
												<button
													type="button"
													onClick={cancelEditing}
													disabled={saving}
													className={`p-1 transition-colors ${
														saving
															? "text-gray-400 cursor-not-allowed"
															: "text-red-600 hover:text-red-800 cursor-pointer"
													}`}
												>
													<X className="w-4 h-4" />
												</button>
											</div>
										) : (
											<div className="min-w-0">
												<div className="flex items-center space-x-2">
													{ipInfo.name ? (
														<span className="text-sm font-medium text-gray-900 truncate">
															{ipInfo.name}
														</span>
													) : (
														<code className="text-sm font-mono text-gray-900">
															{ipInfo.ip_address}
														</code>
													)}
													{ipInfo.proxy && (
														<Badge
															variant="orange"
															pill
															className="flex-shrink-0"
														>
															<Shield className="w-2 h-2 inline mr-1" />
															Proxy
														</Badge>
													)}
													{ipInfo.hosting && (
														<Badge
															variant="purple"
															pill
															className="flex-shrink-0"
														>
															<Wifi className="w-2 h-2 inline mr-1" />
															Host
														</Badge>
													)}
													{(ipInfo.device_count || 0) > 0 && (
														<Badge
															variant="blue"
															pill
															className="flex-shrink-0"
														>
															{ipInfo.device_count} device
															{ipInfo.device_count !== 1 ? "s" : ""}
														</Badge>
													)}
												</div>
												{ipInfo.name && (
													<code className="text-xs font-mono text-gray-600">
														{ipInfo.ip_address}
													</code>
												)}
											</div>
										)}
									</div>

									{/* Right: location, ISP, updated, edit */}
									<div className="flex items-center space-x-4 flex-shrink-0 text-sm">
										<div className="hidden md:flex items-center space-x-1.5 text-gray-600 max-w-xs">
											<CountryFlag
												countryCode={ipInfo.country_code}
												country={ipInfo.country}
											/>
											<span className="truncate">
												{getLocationString(ipInfo)}
												{ipInfo.isp ? ` · ${ipInfo.isp}` : ""}
											</span>
										</div>
										<span className="text-gray-500 tabular-nums">
											{formatTimeAgo(ipInfo.updated_at)}
										</span>
										{!isEditing && (
											<button
												type="button"
												onClick={() => startEditing(ipInfo)}
												className="p-1 text-gray-400 hover:text-gray-600 transition-colors cursor-pointer"
												title="Edit name"
											>
												<Edit2 className="w-4 h-4" />
											</button>
										)}
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

export default IpAddressesPage;
