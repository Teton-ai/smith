import { Toast, type ToastState } from "@teton/smith-ui";
import { Heart } from "lucide-react";
import { useEffect, useState } from "react";
import type { Device } from "@/app/api-client";
import { useFavorites } from "@/app/hooks/favorites";

export default function FavoriteButton({
	device,
	className = "",
}: {
	device: Device;
	className?: string;
}) {
	const { isFavorite, toggle } = useFavorites();
	const [toast, setToast] = useState<ToastState | null>(null);
	const favorite = isFavorite(device.id);

	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	const handleClick = async (e: React.MouseEvent) => {
		// Rendered inside clickable rows/links; the heart must not navigate.
		e.stopPropagation();
		e.preventDefault();
		const ok = await toggle(device);
		if (!ok) {
			setToast({
				message: favorite
					? `Failed to remove ${device.serial_number} from favorites`
					: `Failed to add ${device.serial_number} to favorites`,
				type: "error",
			});
		}
	};

	return (
		<>
			<Toast toast={toast} onClose={() => setToast(null)} />
			<button
				type="button"
				title={favorite ? "Remove from favorites" : "Add to favorites"}
				onClick={handleClick}
				className={`p-1 rounded-md transition-colors cursor-pointer ${
					favorite
						? "text-red-500 hover:text-red-600"
						: "text-gray-300 hover:text-red-400"
				} ${className}`}
			>
				<Heart className={`w-4 h-4 ${favorite ? "fill-current" : ""}`} />
			</button>
		</>
	);
}
