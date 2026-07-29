import { useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface DeviceService {
	id: number;
	release_id: number;
	package_id: number | null;
	service_name: string;
	watchdog_sec: number | null;
	created_at: string;
	active_state: string | null;
	n_restarts: number | null;
	checked_at: string | null;
}

/**
 * systemd states that mean the unit is not running. Mirrors `is_service_down`
 * in `api/src/home.rs`: everything else is indeterminate — smithd reports the
 * literal `"unknown"` when its `systemctl show` call fails, and a service that
 * never reported has no state at all.
 */
const DOWN_STATES = ["failed", "inactive"];

export const isServiceDown = (service: DeviceService) =>
	service.active_state != null && DOWN_STATES.includes(service.active_state);

export const useDeviceServices = (deviceId: string) => {
	const fetcher = useClientMutator<DeviceService[]>();

	return useQuery({
		queryKey: ["deviceServices", deviceId],
		queryFn: () =>
			fetcher({
				url: `/devices/${deviceId}/services`,
				method: "GET",
			}),
		enabled: !!deviceId,
		refetchInterval: 30000,
	});
};

/**
 * The monitored services the device last reported as down. Kept as a hook so
 * the overview can decide whether it has anything to report — `isLoading`
 * matters there, since "all good" shouldn't be claimed before this answers.
 */
export const useDownServices = (deviceId: string) => {
	const { data, isLoading } = useDeviceServices(deviceId);
	return { down: (data ?? []).filter(isServiceDown), isLoading };
};
