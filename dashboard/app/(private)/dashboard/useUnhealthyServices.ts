import { useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface UnhealthyServiceDevice {
	device_id: number;
	serial_number: string;
	service_name: string;
	active_state: string;
	n_restarts: number;
	checked_at: string;
}

export const useUnhealthyServices = () => {
	const fetcher = useClientMutator<UnhealthyServiceDevice[]>();

	return useQuery({
		queryKey: ["dashboardUnhealthyServices"],
		queryFn: () =>
			fetcher({
				url: `/dashboard/unhealthy-services`,
				method: "GET",
			}),
		refetchInterval: 30000,
	});
};
