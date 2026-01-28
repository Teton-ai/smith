"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
	AlertCircle,
	Calendar,
	Check,
	CheckCircle,
	X,
	XCircle,
} from "lucide-react";
import type React from "react";
import { useEffect, useState } from "react";
import {
	type Device,
	useApproveDevice,
	useGetDevices,
	useRevokeDevice,
} from "../../api-client";

export default function ApprovalsPage() {
	const queryClient = useQueryClient();
	const [processingDevices, setProcessingDevices] = useState<Set<number>>(
		new Set(),
	);
	const [toast, setToast] = useState<{
		message: string;
		type: "success" | "error";
	} | null>(null);

	const unapprovedDevices = useGetDevices(
		{ approved: false },
		{ query: { refetchInterval: 5000 } },
	);

	const approveDeviceHook = useApproveDevice();
	const revokeDeviceHook = useRevokeDevice();

	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	const formatTimeAgo = (dateString: string) => {
		const now = new Date();
		const past = new Date(dateString);
		const diff = now.getTime() - past.getTime();
		const minutes = Math.floor(diff / 60000);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days}d ago`;
		if (hours > 0) return `${hours}h ago`;
		return `${minutes}m ago`;
	};

	const getDeviceHostname = (device: Device) => {
		return device.system_info?.hostname || "No hostname";
	};

	const handleApprove = async (deviceId: number, e: React.MouseEvent) => {
		e.stopPropagation();

		const device = unapprovedDevices.data?.find((d) => d.id === deviceId);
		const deviceName = device?.serial_number || "Device";

		setProcessingDevices((prev) => new Set(prev).add(deviceId));

		const success = await approveDeviceHook.mutateAsync({ deviceId });

		if (success) {
			queryClient.invalidateQueries({ queryKey: unapprovedDevices.queryKey });
			setToast({
				message: `${deviceName} approved successfully`,
				type: "success",
			});
		} else {
			setToast({
				message: `Failed to approve ${deviceName}`,
				type: "error",
			});
		}

		setProcessingDevices((prev) => {
			const newSet = new Set(prev);
			newSet.delete(deviceId);
			return newSet;
		});
	};

	const handleReject = async (deviceId: number, e: React.MouseEvent) => {
		e.stopPropagation();

		const device = unapprovedDevices.data?.find((d) => d.id === deviceId);
		const deviceName = device?.serial_number || "Device";

		if (
			!confirm(
				`Are you sure you want to reject ${deviceName}? This will archive it.`,
			)
		) {
			return;
		}

		setProcessingDevices((prev) => new Set(prev).add(deviceId));

		const success = await revokeDeviceHook.mutateAsync({ deviceId });

		if (success) {
			queryClient.invalidateQueries({ queryKey: unapprovedDevices.queryKey });
			setToast({
				message: `${deviceName} rejected and archived`,
				type: "success",
			});
		} else {
			setToast({
				message: `Failed to reject ${deviceName}`,
				type: "error",
			});
		}

		setProcessingDevices((prev) => {
			const newSet = new Set(prev);
			newSet.delete(deviceId);
			return newSet;
		});
	};

	return (
		<>
			{/* Toast Notification */}
			{toast && (
				<div
					className={`fixed top-4 right-4 z-50 p-4 rounded-lg shadow-lg border ${
						toast.type === "success"
							? "bg-green-50 text-green-800 border-green-200"
							: "bg-red-50 text-red-800 border-red-200"
					} transition-all duration-300 ease-in-out`}
				>
					<div className="flex items-center space-x-2">
						{toast.type === "success" ? (
							<Check className="w-5 h-5 text-green-600" />
						) : (
							<X className="w-5 h-5 text-red-600" />
						)}
						<span className="text-sm font-medium">{toast.message}</span>
						<button
							onClick={() => setToast(null)}
							className="ml-2 text-gray-400 hover:text-gray-600"
						>
							<X className="w-4 h-4" />
						</button>
					</div>
				</div>
			)}

			<div className="max-w-4xl mx-auto">
				<div className="mb-6">
					<h1 className="text-2xl font-bold text-gray-900">Pending Approvals</h1>
					<p className="text-sm text-gray-600 mt-1">
						Review and approve or reject devices waiting for access
					</p>
				</div>

				<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
					{unapprovedDevices.isLoading ? (
						<div className="p-6 space-y-4">
							{[...Array(3)].map((_, i) => (
								<div key={i} className="animate-pulse">
									<div className="h-4 bg-gray-200 rounded w-3/4 mb-2" />
									<div className="h-3 bg-gray-200 rounded w-1/2" />
								</div>
							))}
						</div>
					) : unapprovedDevices.data?.length === 0 ? (
						<div className="p-12 text-center">
							<CheckCircle className="w-12 h-12 text-green-500 mx-auto mb-3" />
							<p className="text-lg font-medium text-gray-900 mb-1">
								All caught up!
							</p>
							<p className="text-sm text-gray-600">
								No devices pending approval
							</p>
						</div>
					) : (
						<div className="divide-y divide-gray-200">
							{unapprovedDevices.data?.map((device) => (
								<div
									key={device.id}
									className="p-4 hover:bg-gray-50 transition-colors"
								>
									<div className="flex items-center justify-between">
										<div className="flex items-center space-x-4 min-w-0 flex-1">
											<AlertCircle className="w-5 h-5 text-orange-500 flex-shrink-0" />
											<div className="min-w-0 flex-1">
												<p className="text-sm font-medium text-gray-900">
													{device.serial_number}
												</p>
												<p className="text-sm text-gray-500">
													{getDeviceHostname(device)}
												</p>
												<div className="flex items-center space-x-1 text-xs text-gray-400 mt-1">
													<Calendar className="w-3 h-3" />
													<span>{formatTimeAgo(device.created_on)}</span>
												</div>
											</div>
										</div>
										<div className="flex space-x-2 ml-4">
											<button
												onClick={(e) => handleApprove(device.id, e)}
												disabled={processingDevices.has(device.id)}
												className="flex items-center justify-center space-x-1 px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded-md transition-colors cursor-pointer"
											>
												<CheckCircle className="w-4 h-4" />
												<span>Approve</span>
											</button>
											<button
												onClick={(e) => handleReject(device.id, e)}
												disabled={processingDevices.has(device.id)}
												className="flex items-center justify-center space-x-1 px-4 py-2 text-sm font-medium text-white bg-red-600 hover:bg-red-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded-md transition-colors cursor-pointer"
											>
												<XCircle className="w-4 h-4" />
												<span>Reject</span>
											</button>
										</div>
									</div>
								</div>
							))}
						</div>
					)}
				</div>
			</div>
		</>
	);
}
