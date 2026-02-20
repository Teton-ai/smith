"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface ExtendedTestSessionSummary {
	session_id: string;
	created_at: string;
	device_count: number;
	completed_count: number;
	status: string;
}

export interface SpeedStats {
	average_mbps: number;
	std_dev: number;
	q25: number;
	q50: number;
	q75: number;
}

export interface MinuteStats {
	minute: number;
	sample_count: number;
	download: SpeedStats;
	upload: SpeedStats | null;
}

export interface WifiDetails {
	ssid: string;
	signal_dbm: number;
	frequency_mhz: number;
	vht_mcs: number | null;
	vht_nss: number | null;
	channel_width_mhz: number | null;
}

export interface EthernetDetails {
	speed_mbps: number | null;
	duplex: string | null;
	link_detected: boolean;
}

export interface LteDetails {
	operator: string | null;
	signal_quality: number | null;
	access_technology: string | null;
}

export type NetworkDetails =
	| { Wifi: WifiDetails }
	| { Ethernet: EthernetDetails }
	| { Lte: LteDetails }
	| { Unknown: Record<string, never> };

export interface NetworkInfo {
	interface_type: "Wifi" | "Ethernet" | "Lte" | "Unknown";
	interface_name: string;
	details: NetworkDetails;
}

export interface DeviceExtendedTestResult {
	device_id: number;
	serial_number: string;
	status: string;
	minute_stats: MinuteStats[] | null;
	network_info: NetworkInfo | null;
}

export interface ExtendedTestStatus {
	session_id: string;
	status: string;
	label_filter: string;
	duration_minutes: number;
	device_count: number;
	completed_count: number;
	created_at: string;
	results: DeviceExtendedTestResult[];
}

export const useExtendedTestSessions = () => {
	const fetcher = useClientMutator<ExtendedTestSessionSummary[]>();
	return useQuery({
		queryKey: ["extendedTestSessions"],
		queryFn: () =>
			fetcher({ url: "/network/extended-test/sessions", method: "GET" }),
	});
};

export const useExtendedTestStatus = (sessionId: string | null) => {
	const fetcher = useClientMutator<ExtendedTestStatus>();
	return useQuery({
		queryKey: ["extendedTestStatus", sessionId],
		queryFn: () =>
			fetcher({
				url: `/network/extended-test/${sessionId}`,
				method: "GET",
			}),
		enabled: !!sessionId,
		refetchInterval: (query) =>
			query.state.data?.status === "running" ||
			query.state.data?.status === "pending" ||
			query.state.data?.status === "partial"
				? 5000
				: false,
	});
};

export interface StartExtendedTestRequest {
	label_filter: string;
	duration_minutes: number;
}

export interface StartExtendedTestResponse {
	session_id: string;
	device_count: number;
	message: string;
}

export const useStartExtendedTest = () => {
	const fetcher = useClientMutator<StartExtendedTestResponse>();
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (request: StartExtendedTestRequest) =>
			fetcher({
				url: "/network/extended-test",
				method: "POST",
				data: request,
			}),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: ["extendedTestSessions"] });
		},
	});
};

// Types for online devices check
interface ConnectionStatus {
	connection_name: string;
	connection_state: string;
	device_name: string;
	device_type: string;
}

interface OnlineDevice {
	id: number;
	serial_number: string;
	system_info?: {
		connection_statuses?: ConnectionStatus[];
	};
}

interface OnlineDevicesResponse {
	devices: OnlineDevice[];
}

export interface DongleInfo {
	totalOnlineDevices: number;
	dongleDevices: { id: number; serial_number: string }[];
}

export interface CancelExtendedTestResponse {
	canceled_count: number;
	message: string;
}

export const useCancelExtendedTest = () => {
	const fetcher = useClientMutator<CancelExtendedTestResponse>();
	const queryClient = useQueryClient();

	return useMutation({
		mutationFn: (sessionId: string) =>
			fetcher({
				url: `/network/extended-test/${sessionId}/cancel`,
				method: "POST",
			}),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: ["extendedTestSessions"] });
			queryClient.invalidateQueries({ queryKey: ["extendedTestStatus"] });
		},
	});
};

export const useOnlineDevicesDongleCheck = () => {
	const fetcher = useClientMutator<OnlineDevicesResponse>();

	return useQuery({
		queryKey: ["onlineDevicesDongleCheck"],
		queryFn: async (): Promise<DongleInfo> => {
			const response = await fetcher({
				url: "/devices?online=true",
				method: "GET",
			});

			const dongleDevices = response.devices.filter((device) => {
				const connectedStatuses = device.system_info?.connection_statuses?.filter(
					(conn) => conn.connection_state === "connected"
				);
				// Check if any connected interface is gsm (cellular/dongle)
				return connectedStatuses?.some((conn) => conn.device_type === "gsm");
			});

			return {
				totalOnlineDevices: response.devices.length,
				dongleDevices: dongleDevices.map((d) => ({
					id: d.id,
					serial_number: d.serial_number,
				})),
			};
		},
		staleTime: 30000, // Cache for 30 seconds
	});
};
