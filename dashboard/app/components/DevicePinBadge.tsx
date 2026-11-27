import { Badge } from "@teton/smith-ui";
import { Pin } from "lucide-react";
import { Link, useLocation } from "react-router";
import type { Device } from "@/app/api-client";
import { Tooltip } from "@/app/components/tooltip";

/** Pinning controls target assignment, independently of update progress. */
export function DevicePinBadge({
	device,
	plain = false,
}: {
	device: Device;
	plain?: boolean;
}) {
	const location = useLocation();
	if (device.follow_latest !== false) return null;

	const target = device.target_release;
	const targetLabel = target
		? `v${target.version}`
		: device.target_release_id != null
			? `release #${device.target_release_id}`
			: null;
	const description = targetLabel
		? `Pinned to ${targetLabel}. Won't update automatically.`
		: "Pinned. No target release assigned. Won't update automatically.";

	return (
		<Tooltip
			content={
				<span className="block w-64 whitespace-normal">{description}</span>
			}
			placement={plain ? "top" : "right"}
			className="z-10 hover:z-20 focus-within:z-20 shrink-0"
		>
			{plain ? (
				<button
					type="button"
					aria-label={description}
					className="flex items-center gap-1 cursor-help rounded focus-visible:outline-2 focus-visible:outline-blue-600"
				>
					<Pin className="w-4 h-4" aria-hidden="true" />
					<span>Pinned</span>
				</button>
			) : (
				<Link
					to={`/devices/${device.serial_number}`}
					state={{ back: location.pathname + location.search }}
					aria-label={`Open ${device.serial_number}. ${description}`}
					className="rounded-full focus-visible:outline-2 focus-visible:outline-blue-600"
				>
					<Badge variant="gray" pill className="inline-flex items-center gap-1">
						<Pin className="w-3 h-3" aria-hidden="true" />
						Pinned
					</Badge>
				</Link>
			)}
		</Tooltip>
	);
}
