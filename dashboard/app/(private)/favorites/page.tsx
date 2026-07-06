import { Card, CountryFlag, PageContainer } from "@teton/smith-ui";
import { Heart } from "lucide-react";
import { useNavigate } from "react-router";
import FavoriteButton from "@/app/components/FavoriteButton";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import { useFavorites } from "@/app/hooks/favorites";
import { formatTimeAgo, sortByLastSeen } from "@/app/utils/device";

const FavoriteSkeleton = () => (
	<div className="flex items-center justify-between px-4 py-3 animate-pulse">
		<div className="flex items-center space-x-3">
			<div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0" />
			<div className="h-4 bg-gray-300 rounded w-64" />
		</div>
		<div className="h-4 bg-gray-200 rounded w-20" />
	</div>
);

const FavoritesPage = () => {
	const navigate = useNavigate();
	const { favorites, isLoading } = useFavorites();

	const sortedFavorites = [...favorites].sort(sortByLastSeen);

	return (
		<PageContainer>
			<div className="flex items-center justify-between">
				<h1 className="text-xl font-semibold text-gray-900">Favorites</h1>
				<span className="text-sm text-gray-500">
					{isLoading
						? "Loading..."
						: `${favorites.length} device${favorites.length !== 1 ? "s" : ""}`}
				</span>
			</div>

			<Card className="overflow-hidden">
				{isLoading ? (
					<div className="divide-y divide-gray-100">
						{Array.from({ length: 4 }, (_, i) => (
							<FavoriteSkeleton key={i} />
						))}
					</div>
				) : sortedFavorites.length === 0 ? (
					<div className="p-12 text-center text-gray-500">
						<Heart className="w-12 h-12 text-gray-400 mx-auto mb-4" />
						<h3 className="text-lg font-medium text-gray-900 mb-2">
							No favorite devices yet
						</h3>
						<p className="text-gray-500">
							Click the heart icon on a device to add it to your favorites.
						</p>
					</div>
				) : (
					<div className="divide-y divide-gray-100">
						{/* Rows navigate via onClick (devices-page idiom) rather than a
						    Link so the heart button is not nested inside an anchor. */}
						{sortedFavorites.map((device) => (
							// biome-ignore lint/a11y/useSemanticElements: can't use <button> because FavoriteButton (also a <button>) is a child
							<div
								key={device.id}
								role="button"
								tabIndex={0}
								className="flex items-center justify-between px-4 py-3 hover:bg-gray-50 cursor-pointer transition-colors"
								onClick={() => navigate(`/devices/${device.serial_number}`)}
								onKeyDown={(e) => {
									if (e.key === "Enter" || e.key === " ") {
										e.preventDefault();
										navigate(`/devices/${device.serial_number}`);
									}
								}}
							>
								<div className="flex items-center space-x-3 min-w-0">
									<NetworkQualityIndicator
										isOnline={device.online}
										networkScore={device.network?.network_score}
									/>
									<CountryFlag
										countryCode={device.ip_address?.country_code}
										country={device.ip_address?.country}
									/>
									<span className="font-mono text-sm text-gray-900 truncate">
										{device.serial_number}
									</span>
								</div>
								<div className="flex items-center space-x-3 flex-shrink-0 text-sm">
									<span className="text-gray-500">
										{device.last_seen
											? formatTimeAgo(device.last_seen)
											: "never"}
									</span>
									<FavoriteButton device={device} />
								</div>
							</div>
						))}
					</div>
				)}
			</Card>
		</PageContainer>
	);
};

export default FavoritesPage;
