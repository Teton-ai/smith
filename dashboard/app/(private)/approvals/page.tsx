"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
	ArrowLeft,
	Check,
	CheckCircle,
	Layers,
	Loader2,
	X,
	XCircle,
} from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import {
	type Device,
	type Release,
	useApproveDevice,
	useDeleteDevice,
	useGetDevices,
	useGetReleases,
	useUpdateDeviceTargetRelease,
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

	// Modal state
	const [mounted, setMounted] = useState(false);
	const [approveModalDevice, setApproveModalDevice] = useState<Device | null>(
		null,
	);
	const [selectedDistribution, setSelectedDistribution] = useState<
		string | null
	>(null);

	useEffect(() => {
		setMounted(true);
	}, []);

	const unapprovedDevices = useGetDevices(
		{ approved: false },
		{ query: { refetchInterval: 5000 } },
	);

	const { data: allReleases = [] } = useGetReleases();

	const approveDeviceHook = useApproveDevice();
	const deleteDeviceHook = useDeleteDevice();
	const updateTargetRelease = useUpdateDeviceTargetRelease();

	useEffect(() => {
		if (toast) {
			const timer = setTimeout(() => setToast(null), 3000);
			return () => clearTimeout(timer);
		}
	}, [toast]);

	// Group stable releases by distribution, sorted newest first
	const distributionMap = useMemo(() => {
		const grouped: Record<
			string,
			{ id: number; latestRelease: Release; count: number }
		> = {};
		const sorted = [...allReleases]
			.filter((r) => !r.draft && !r.yanked)
			.sort(
				(a, b) =>
					new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
			);
		for (const release of sorted) {
			const distName = release.distribution_name || "Unknown";
			if (!grouped[distName]) {
				grouped[distName] = {
					id: release.distribution_id,
					latestRelease: release,
					count: 1,
				};
			} else {
				grouped[distName].count++;
			}
		}
		return grouped;
	}, [allReleases]);

	const selectedLatestRelease = selectedDistribution
		? distributionMap[selectedDistribution]?.latestRelease
		: null;

	const formatRelativeTime = (dateString: string) => {
		return moment(dateString).fromNow();
	};

	const getDeviceHostname = (device: Device) => {
		return device.system_info?.hostname || "No hostname";
	};

	const handleApproveAndAssign = async () => {
		if (!approveModalDevice || !selectedLatestRelease) return;

		const device = approveModalDevice;
		const deviceName = device.serial_number || "Device";

		setProcessingDevices((prev) => new Set(prev).add(device.id));
		setApproveModalDevice(null);

		try {
			await approveDeviceHook.mutateAsync({ deviceId: device.id });

			await updateTargetRelease.mutateAsync({
				deviceId: device.id,
				data: { target_release_id: selectedLatestRelease.id },
			});

			queryClient.invalidateQueries({ queryKey: unapprovedDevices.queryKey });
			setToast({
				message: `${deviceName} approved → ${selectedDistribution} ${selectedLatestRelease.version}`,
				type: "success",
			});
		} catch {
			setToast({
				message: `Failed to approve ${deviceName}`,
				type: "error",
			});
		} finally {
			setProcessingDevices((prev) => {
				const newSet = new Set(prev);
				newSet.delete(device.id);
				return newSet;
			});
			setSelectedDistribution(null);
		}
	};

	const handleReject = async (device: Device) => {
		const deviceName = device.serial_number || "Device";

		if (
			!confirm(
				`Are you sure you want to reject ${deviceName}? This will archive it.`,
			)
		) {
			return;
		}

		setProcessingDevices((prev) => new Set(prev).add(device.id));

		try {
			await deleteDeviceHook.mutateAsync({ deviceId: device.id });
			queryClient.invalidateQueries({ queryKey: unapprovedDevices.queryKey });
			setToast({
				message: `${deviceName} rejected and archived`,
				type: "success",
			});
		} catch {
			setToast({
				message: `Failed to reject ${deviceName}`,
				type: "error",
			});
		}

		setProcessingDevices((prev) => {
			const newSet = new Set(prev);
			newSet.delete(device.id);
			return newSet;
		});
	};

	const loading = unapprovedDevices.isLoading;
	const devices = unapprovedDevices.data || [];

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
							className="ml-2 text-gray-400 hover:text-gray-600 cursor-pointer"
						>
							<X className="w-4 h-4" />
						</button>
					</div>
				</div>
			)}

			{/* Back link */}
			<Link
				href="/dashboard"
				className="inline-flex items-center text-sm text-gray-500 hover:text-gray-700 mb-4"
			>
				<ArrowLeft className="w-4 h-4 mr-1" />
				Back to Dashboard
			</Link>

			{/* Content */}
			{loading ? (
				<div className="bg-white rounded-lg border border-gray-200">
					<div className="px-4 py-3 border-b border-gray-200">
						<div className="h-5 bg-gray-200 rounded w-40 animate-pulse" />
					</div>
					{[...Array(5)].map((_, i) => (
						<div
							key={i}
							className="px-4 py-3 border-b border-gray-200 last:border-b-0"
						>
							<div className="flex items-center justify-between">
								<div className="flex items-center space-x-3">
									<div className="h-4 bg-gray-200 rounded w-64 animate-pulse" />
									<div className="h-3 bg-gray-200 rounded w-24 animate-pulse" />
								</div>
								<div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
							</div>
						</div>
					))}
				</div>
			) : devices.length === 0 ? (
				<div className="bg-white rounded-lg border border-gray-200 p-8 text-center">
					<CheckCircle className="w-10 h-10 text-green-500 mx-auto mb-2" />
					<p className="text-sm font-medium text-gray-900 mb-1">
						All caught up!
					</p>
					<p className="text-xs text-gray-500">No devices pending approval</p>
				</div>
			) : (
				<div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
					<div className="bg-orange-50 px-4 py-3 border-b border-gray-200">
						<h4 className="text-sm font-semibold text-orange-800">
							Pending Approval ({devices.length})
						</h4>
					</div>
					<div className="divide-y divide-gray-200">
						{devices.map((device) => (
							<div
								key={device.id}
								className="px-4 py-3 hover:bg-gray-50 transition-colors"
							>
								<div className="flex items-center justify-between">
									<div className="flex items-center space-x-4 min-w-0 flex-1">
										<span className="font-mono text-sm text-gray-900 truncate">
											{device.serial_number}
										</span>
										<span className="text-xs text-gray-400 whitespace-nowrap hidden sm:inline">
											{getDeviceHostname(device)}
										</span>
										<span className="text-xs text-gray-400 whitespace-nowrap hidden md:inline">
											{formatRelativeTime(device.created_on)}
										</span>
									</div>
									<div className="flex items-center space-x-2 flex-shrink-0 ml-4">
										<button
											onClick={() => {
												setApproveModalDevice(device);
												setSelectedDistribution(null);
											}}
											disabled={processingDevices.has(device.id)}
											className="flex items-center space-x-1 px-3 py-1.5 text-xs font-medium text-white bg-green-600 hover:bg-green-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded transition-colors cursor-pointer"
										>
											{processingDevices.has(device.id) ? (
												<Loader2 className="w-3 h-3 animate-spin" />
											) : (
												<CheckCircle className="w-3 h-3" />
											)}
											<span>Approve</span>
										</button>
										<button
											onClick={() => handleReject(device)}
											disabled={processingDevices.has(device.id)}
											className="flex items-center space-x-1 px-3 py-1.5 text-xs font-medium text-white bg-red-600 hover:bg-red-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded transition-colors cursor-pointer"
										>
											<XCircle className="w-3 h-3" />
											<span>Reject</span>
										</button>
									</div>
								</div>
							</div>
						))}
					</div>
				</div>
			)}

			{/* Approve & Assign Distribution Modal */}
			{mounted &&
				approveModalDevice &&
				createPortal(
					<div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
						<div className="bg-white rounded-lg shadow-xl p-6 w-[520px] animate-in zoom-in-95 duration-200">
							<div className="flex justify-between items-center mb-4">
								<div>
									<h2 className="text-xl font-semibold text-gray-900">
										Approve & Assign Distribution
									</h2>
									<p className="text-sm text-gray-500 mt-0.5">
										{approveModalDevice.serial_number}
									</p>
								</div>
								<button
									onClick={() => setApproveModalDevice(null)}
									className="text-gray-400 hover:text-gray-600 cursor-pointer"
								>
									<X className="w-5 h-5" />
								</button>
							</div>

							<label className="block text-sm font-medium text-gray-700 mb-3">
								Select a distribution
							</label>

							{Object.keys(distributionMap).length === 0 ? (
								<div className="text-center py-6 text-gray-500 text-sm border border-gray-200 rounded-md mb-6">
									No distributions with stable releases available
								</div>
							) : (
								<div className="space-y-2 max-h-64 overflow-y-auto mb-6">
									{Object.entries(distributionMap).map(
										([distName, { latestRelease, count }]) => (
											<button
												key={distName}
												onClick={() => setSelectedDistribution(distName)}
												className={`w-full text-left p-4 rounded-lg border transition-colors cursor-pointer ${
													selectedDistribution === distName
														? "border-indigo-500 bg-indigo-50"
														: "border-gray-200 hover:border-gray-300 hover:bg-gray-50"
												}`}
											>
												<div className="flex items-center justify-between">
													<div className="flex items-center space-x-3">
														<Layers
															className={`w-5 h-5 ${
																selectedDistribution === distName
																	? "text-indigo-600"
																	: "text-gray-400"
															}`}
														/>
														<div>
															<p className="font-medium text-gray-900">
																{distName}
															</p>
															<p className="text-xs text-gray-500 mt-0.5">
																Latest: {latestRelease.version} ·{" "}
																{formatRelativeTime(latestRelease.created_at)} ·{" "}
																{count} release{count !== 1 ? "s" : ""}
															</p>
														</div>
													</div>
													{selectedDistribution === distName && (
														<div className="w-5 h-5 bg-indigo-600 rounded-full flex items-center justify-center flex-shrink-0">
															<svg
																className="w-3 h-3 text-white"
																fill="none"
																viewBox="0 0 24 24"
																stroke="currentColor"
															>
																<path
																	strokeLinecap="round"
																	strokeLinejoin="round"
																	strokeWidth={2}
																	d="M5 13l4 4L19 7"
																/>
															</svg>
														</div>
													)}
												</div>
											</button>
										),
									)}
								</div>
							)}

							<div className="flex justify-end space-x-3">
								<button
									onClick={() => setApproveModalDevice(null)}
									className="px-4 py-2 text-sm font-medium text-gray-700 bg-white border border-gray-300 rounded-md hover:bg-gray-50 transition-colors cursor-pointer"
								>
									Cancel
								</button>
								<button
									onClick={handleApproveAndAssign}
									disabled={!selectedLatestRelease}
									className="flex items-center space-x-2 px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded-md transition-colors cursor-pointer"
								>
									<CheckCircle className="w-4 h-4" />
									<span>
										{selectedLatestRelease
											? `Approve → ${selectedLatestRelease.version}`
											: "Approve & Assign"}
									</span>
								</button>
							</div>
						</div>
					</div>,
					document.body,
				)}
		</>
	);
}
