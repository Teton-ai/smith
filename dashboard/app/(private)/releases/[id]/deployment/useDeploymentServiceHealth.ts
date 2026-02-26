import { useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface DeviceServiceHealth {
	device_id: number;
	serial_number: string;
	release_service_id: number;
	service_name: string;
	active_state: string;
	n_restarts: number;
	checked_at: string;
}

export const useDeploymentServiceHealth = (releaseId: number) => {
	const fetcher = useClientMutator<DeviceServiceHealth[]>();

	return useQuery({
		queryKey: ["deploymentServiceHealth", releaseId],
		queryFn: () =>
			fetcher({
				url: `/releases/${releaseId}/deployment/service-health`,
				method: "GET",
			}),
		enabled: !!releaseId,
		refetchInterval: 5000,
	});
};
