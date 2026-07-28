import { useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface ServiceOutage {
	service_name: string;
	started_at: string;
	/** Null while the outage is still open. */
	ended_at: string | null;
}

export interface DeviceUptime {
	from: string;
	to: string;
	services: string[];
	outages: ServiceOutage[];
}

export const UPTIME_RANGES = [
	{ label: "1h", hours: 1 },
	{ label: "6h", hours: 6 },
	{ label: "24h", hours: 24 },
	{ label: "7d", hours: 24 * 7 },
] as const;

export type UptimeRange = (typeof UPTIME_RANGES)[number]["label"];

export const useDeviceUptime = (deviceId: string, hours: number) => {
	const fetcher = useClientMutator<DeviceUptime>();

	return useQuery({
		queryKey: ["deviceUptime", deviceId, hours],
		queryFn: () => {
			// Anchored per fetch rather than per render so the window doesn't shift
			// on every re-render and defeat the query cache.
			const to = new Date();
			const from = new Date(to.getTime() - hours * 3600_000);
			return fetcher({
				url: `/devices/${deviceId}/uptime`,
				method: "GET",
				params: { from: from.toISOString(), to: to.toISOString() },
			});
		},
		enabled: !!deviceId,
		refetchInterval: 60000,
	});
};
