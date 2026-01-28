import { useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

interface DeviceService {
	id: number;
	release_id: number;
	package_id: number | null;
	service_name: string;
	watchdog_sec: number | null;
	created_at: string;
}

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
	});
};
