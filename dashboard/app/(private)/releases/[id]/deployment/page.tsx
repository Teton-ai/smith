"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
	Activity,
	ArrowLeft,
	CheckCircle2,
	Clock,
	Loader2,
	Monitor,
	Rocket,
	XCircle,
} from "lucide-react";
import moment from "moment";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import {
	type Deployment,
	type DeploymentDeviceWithStatus,
	useApiConfirmFullRollout,
	useApiGetDeploymentDevices,
	useApiGetReleaseDeployment,
	useGetRelease,
} from "@/app/api-client";

const DeploymentStatusPage = () => {
	const params = useParams();
	const releaseId = parseInt(params.id as string);
	const queryClient = useQueryClient();
	const [elapsedTime, setElapsedTime] = useState(0);

	const { data: release, isLoading: releaseLoading } = useGetRelease(releaseId);

	const { data: deployment, queryKey: deploymentQueryKey } =
		useApiGetReleaseDeployment(releaseId);

	const { data: devices = [], queryKey: devicesQueryKey } =
		useApiGetDeploymentDevices(releaseId);

	const loading = releaseLoading;

	useEffect(() => {
		if (!deployment) return;

		const timer = setInterval(() => {
			const start = new Date(deployment.created_at).getTime();
			const now = Date.now();
			setElapsedTime(Math.floor((now - start) / 1000));
		}, 1000);

		return () => clearInterval(timer);
	}, [deployment]);

	const formatElapsedTime = (seconds: number) => {
		const mins = Math.floor(seconds / 60);
		const secs = seconds % 60;
		return `${mins}m ${secs}s`;
	};

	const isCanaryComplete = () => {
		if (
			!deployment ||
			deployment.status !== "InProgress" ||
			devices.length === 0
		) {
			return false;
		}
		return devices.every(
			(device) => device.release_id === device.target_release_id,
		);
	};

	const confirmFullRolloutHook = useApiConfirmFullRollout({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: deploymentQueryKey });
				queryClient.invalidateQueries({ queryKey: devicesQueryKey });
			},
		},
	});

	const getDeviceStatus = (device: DeploymentDeviceWithStatus) => {
		if (device.release_id === device.target_release_id) {
			return {
				status: "updated",
				label: "Updated",
				color: "text-green-700 bg-green-100 border-green-200",
			};
		}
		if (
			device.last_ping &&
			moment(device.last_ping).isAfter(moment().subtract(5, "minutes"))
		) {
			return {
				status: "updating",
				label: "Updating...",
				color: "text-blue-700 bg-blue-100 border-blue-200",
			};
		}
		return {
			status: "pending",
			label: "Pending",
			color: "text-gray-700 bg-gray-100 border-gray-200",
		};
	};

	const getStatusIcon = (status: Deployment["status"]) => {
		switch (status) {
			case "InProgress":
				return <Loader2 className="w-6 h-6 text-blue-600 animate-spin" />;
			case "Done":
				return <CheckCircle2 className="w-6 h-6 text-green-600" />;
			case "Failed":
				return <XCircle className="w-6 h-6 text-red-600" />;
			case "Canceled":
				return <XCircle className="w-6 h-6 text-gray-600" />;
			default:
				return <Clock className="w-6 h-6 text-gray-400" />;
		}
	};

	const getStatusText = (status: Deployment["status"]) => {
		switch (status) {
			case "InProgress":
				return "IN PROGRESS";
			case "Done":
				return "COMPLETED";
			case "Failed":
				return "FAILED";
			case "Canceled":
				return "CANCELED";
			default:
				return "UNKNOWN";
		}
	};

	const getStatusColor = (status: Deployment["status"]) => {
		switch (status) {
			case "InProgress":
				return "text-blue-700 bg-blue-100 border-blue-200";
			case "Done":
				return "text-green-700 bg-green-100 border-green-200";
			case "Failed":
				return "text-red-700 bg-red-100 border-red-200";
			case "Canceled":
				return "text-gray-700 bg-gray-100 border-gray-200";
			default:
				return "text-gray-700 bg-gray-100 border-gray-200";
		}
	};

	if (loading) {
		return (
			<div className="flex items-center justify-center h-32">
				<div className="text-gray-500 text-sm">Loading...</div>
			</div>
		);
	}

	if (!release) {
		return (
			<div className="flex items-center justify-center h-32">
				<div className="text-red-500 text-sm">Error: Release not found</div>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<div className="flex items-center space-x-4">
				<Link
					href={`/releases/${releaseId}`}
					className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
				>
					<ArrowLeft className="w-4 h-4" />
					<span className="text-sm font-medium">Back to Release</span>
				</Link>
			</div>

			<div className="bg-white rounded-lg border border-gray-200 p-6">
				<div className="flex items-start justify-between mb-6">
					<div>
						<h1 className="text-2xl font-bold text-gray-900 mb-2">
							Deploying Release v{release.version}
						</h1>
						<div className="flex items-center space-x-4 text-sm text-gray-600">
							<span>{release.distribution_name}</span>
							<span className="px-2 py-1 text-xs font-medium bg-gray-100 text-gray-700 rounded-full">
								{release.distribution_architecture.toUpperCase()}
							</span>
						</div>
					</div>
					{deployment && (
						<div className="flex items-center space-x-2">
							{getStatusIcon(deployment.status)}
							<span
								className={`px-3 py-1 text-sm font-semibold rounded-full border ${getStatusColor(deployment.status)}`}
							>
								{getStatusText(deployment.status)}
							</span>
						</div>
					)}
				</div>

				{deployment && (
					<>
						<div className="mb-6 flex items-center space-x-6 text-sm text-gray-600">
							<div className="flex items-center space-x-2">
								<Clock className="w-4 h-4" />
								<span>Started {moment(deployment.created_at).fromNow()}</span>
							</div>
							{deployment.status === "InProgress" && (
								<div className="flex items-center space-x-2">
									<Activity className="w-4 h-4" />
									<span>Elapsed: {formatElapsedTime(elapsedTime)}</span>
								</div>
							)}
							{deployment.status === "Done" && (
								<div className="flex items-center space-x-2">
									<CheckCircle2 className="w-4 h-4 text-green-600" />
									<span>
										Completed in{" "}
										{formatElapsedTime(
											Math.floor(
												(new Date(deployment.updated_at).getTime() -
													new Date(deployment.created_at).getTime()) /
													1000,
											),
										)}
									</span>
								</div>
							)}
						</div>

						<div className="space-y-4">
							<div
								className={`border rounded-lg p-4 ${
									deployment.status === "InProgress"
										? "border-blue-200 bg-blue-50"
										: deployment.status === "Done"
											? "border-green-200 bg-green-50"
											: "border-gray-200 bg-gray-50"
								}`}
							>
								<div className="flex items-center justify-between mb-3">
									<div className="flex items-center space-x-2">
										<Rocket
											className={`w-5 h-5 ${
												deployment.status === "InProgress"
													? "text-blue-600"
													: deployment.status === "Done"
														? "text-green-600"
														: "text-gray-600"
											}`}
										/>
										<h3
											className={`font-semibold ${
												deployment.status === "InProgress"
													? "text-blue-900"
													: deployment.status === "Done"
														? "text-green-900"
														: "text-gray-900"
											}`}
										>
											{deployment.status === "InProgress"
												? "Deployment in Progress"
												: deployment.status === "Done"
													? "Deployment Complete"
													: "Deployment Status"}
										</h3>
									</div>
								</div>
								<div
									className={`text-sm ${
										deployment.status === "InProgress"
											? "text-blue-800"
											: deployment.status === "Done"
												? "text-green-800"
												: "text-gray-800"
									}`}
								>
									{deployment.status === "InProgress" && (
										<div className="space-y-2">
											<p className="font-medium">Phase 1: Canary Deployment</p>
											{isCanaryComplete() ? (
												<>
													<p>
														All canary devices have been successfully updated!
													</p>
													<div className="mt-4">
														<button
															onClick={() => {
																confirmFullRolloutHook.mutate({ releaseId });
															}}
															disabled={confirmFullRolloutHook.isPending}
															className="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed flex items-center space-x-2 transition-colors"
														>
															{confirmFullRolloutHook.isPending ? (
																<>
																	<Loader2 className="w-4 h-4 animate-spin" />
																	<span>Confirming...</span>
																</>
															) : (
																<>
																	<CheckCircle2 className="w-4 h-4" />
																	<span>Confirm Full Rollout</span>
																</>
															)}
														</button>
													</div>
												</>
											) : (
												<>
													<p>
														Deploying to ~10 recently active devices. Please
														confirm full rollout once all canary devices have
														successfully updated.
													</p>
													<div className="mt-3 flex items-center space-x-2">
														<Loader2 className="w-4 h-4 animate-spin" />
														<span>Waiting for devices to update...</span>
													</div>
												</>
											)}
										</div>
									)}
									{deployment.status === "Done" && (
										<div className="space-y-2">
											<p className="font-medium">All phases complete!</p>
											<p>
												The release has been successfully deployed. All devices
												in this distribution will update to v{release.version}{" "}
												as they come online.
											</p>
										</div>
									)}
									{deployment.status === "Failed" && (
										<div className="space-y-2">
											<p className="font-medium">Deployment failed</p>
											<p>
												The deployment encountered an error. Please check the
												device logs for more information.
											</p>
										</div>
									)}
								</div>
							</div>

							{deployment.status === "InProgress" && !isCanaryComplete() && (
								<div className="border border-gray-200 bg-gray-50 rounded-lg p-4">
									<div className="flex items-center space-x-2 mb-3">
										<Clock className="w-5 h-5 text-gray-600" />
										<h3 className="font-semibold text-gray-900">
											Phase 2: Full Rollout
										</h3>
									</div>
									<p className="text-sm text-gray-700">
										Waiting for canary deployment to complete. You will need to
										manually confirm to proceed with full rollout.
									</p>
								</div>
							)}
						</div>

						{deployment.status === "InProgress" && (
							<div className="mt-6 p-4 bg-yellow-50 border border-yellow-200 rounded-lg">
								<p className="text-sm text-yellow-800">
									<strong>Note:</strong> This page will automatically refresh
									every 5 seconds to show the latest status.{" "}
									{isCanaryComplete()
										? "Please confirm full rollout to proceed."
										: "You can safely navigate away and return later."}
								</p>
							</div>
						)}

						{devices.length > 0 && (
							<div className="mt-6">
								<div className="flex items-center space-x-2 mb-4">
									<Monitor className="w-5 h-5 text-gray-700" />
									<h3 className="font-semibold text-gray-900">
										Deployment Devices ({devices.length})
									</h3>
								</div>
								<div className="bg-white border border-gray-200 rounded-lg overflow-hidden">
									<table className="min-w-full divide-y divide-gray-200">
										<thead className="bg-gray-50">
											<tr>
												<th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
													Serial Number
												</th>
												<th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
													Status
												</th>
												<th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
													Last Ping
												</th>
												<th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
													Added
												</th>
											</tr>
										</thead>
										<tbody className="bg-white divide-y divide-gray-200">
											{devices.map((device) => {
												const deviceStatus = getDeviceStatus(device);
												return (
													<tr
														key={device.device_id}
														className="hover:bg-gray-50"
													>
														<td className="px-4 py-3 text-sm font-mono text-gray-900">
															{device.serial_number}
														</td>
														<td className="px-4 py-3 text-sm">
															<span
																className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium border ${deviceStatus.color}`}
															>
																{deviceStatus.status === "updated" && (
																	<CheckCircle2 className="w-3 h-3 mr-1" />
																)}
																{deviceStatus.status === "updating" && (
																	<Loader2 className="w-3 h-3 mr-1 animate-spin" />
																)}
																{deviceStatus.status === "pending" && (
																	<Clock className="w-3 h-3 mr-1" />
																)}
																{deviceStatus.label}
															</span>
														</td>
														<td className="px-4 py-3 text-sm text-gray-600">
															{device.last_ping
																? moment(device.last_ping).fromNow()
																: "Never"}
														</td>
														<td className="px-4 py-3 text-sm text-gray-600">
															{moment(device.added_at).fromNow()}
														</td>
													</tr>
												);
											})}
										</tbody>
									</table>
								</div>
							</div>
						)}
					</>
				)}

				{!deployment && (
					<div className="text-center py-8">
						<p className="text-gray-500">
							No deployment found for this release.
						</p>
					</div>
				)}
			</div>
		</div>
	);
};

export default DeploymentStatusPage;
