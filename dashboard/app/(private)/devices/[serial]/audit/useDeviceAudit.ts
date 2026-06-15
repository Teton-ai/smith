import { useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface DeviceAudit {
	disk_encrypted: boolean | null;
	password_access_disabled: boolean | null;
	running_latest_release: boolean;
	checked_at: string | null;
}

export const useDeviceAudit = (deviceId: string) => {
	const fetcher = useClientMutator<DeviceAudit>();

	return useQuery({
		queryKey: ["deviceAudit", deviceId],
		queryFn: () =>
			fetcher({
				url: `/devices/${deviceId}/audit`,
				method: "GET",
			}),
		enabled: !!deviceId,
		refetchInterval: 30000,
	});
};
