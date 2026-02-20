"use client";

import { Activity, Calendar, ChevronDown, Loader2, Play, RefreshCw } from "lucide-react";
import { useSearchParams, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import DepartmentOverview from "./components/DepartmentOverview";
import DeviceInspector from "./components/DeviceInspector";
import DeviceTable from "./components/DeviceTable";
import InsightsCards from "./components/InsightsCards";
import StartTestModal from "./components/StartTestModal";
import {
	type DeviceExtendedTestResult,
	useStartExtendedTest,
	useExtendedTestSessions,
	useExtendedTestStatus,
} from "./hooks/useExtendedTest";

function formatSessionDate(dateString: string): string {
	const date = new Date(dateString);
	return date.toLocaleDateString("en-US", {
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

export default function HackathonPage() {
	const router = useRouter();
	const searchParams = useSearchParams();

	const [selectedSessionId, setSelectedSessionId] = useState<string | null>(
		searchParams.get("session") || null
	);
	const [showSessionDropdown, setShowSessionDropdown] = useState(false);
	const [selectedDevice, setSelectedDevice] = useState<DeviceExtendedTestResult | null>(null);
	const [showStartModal, setShowStartModal] = useState(false);
	const [durationMinutes, setDurationMinutes] = useState(3);
	const [labelFilter, setLabelFilter] = useState("");

	const {
		data: sessions,
		isLoading: sessionsLoading,
		refetch: refetchSessions,
	} = useExtendedTestSessions();

	const {
		data: sessionStatus,
		isLoading: statusLoading,
		refetch: refetchStatus,
	} = useExtendedTestStatus(selectedSessionId);

	const startTestMutation = useStartExtendedTest();

	// Auto-select the latest session if none selected
	useEffect(() => {
		if (!selectedSessionId && sessions && sessions.length > 0) {
			const latestSession = sessions[0];
			setSelectedSessionId(latestSession.session_id);
			router.replace(`/hackathon?session=${latestSession.session_id}`);
		}
	}, [sessions, selectedSessionId, router]);

	const handleSessionSelect = (sessionId: string) => {
		setSelectedSessionId(sessionId);
		setSelectedDevice(null);
		setShowSessionDropdown(false);
		router.replace(`/hackathon?session=${sessionId}`);
	};

	const handleStartTest = async () => {
		try {
			const result = await startTestMutation.mutateAsync({
				label_filter: labelFilter,
				duration_minutes: durationMinutes,
			});
			setShowStartModal(false);
			setSelectedSessionId(result.session_id);
			router.replace(`/hackathon?session=${result.session_id}`);
		} catch (error) {
			console.error("Failed to start test:", error);
		}
	};

	const selectedSession = sessions?.find((s) => s.session_id === selectedSessionId);
	const isTestRunning = sessionStatus?.status === "running" || sessionStatus?.status === "pending" || sessionStatus?.status === "partial";

	if (sessionsLoading) {
		return (
			<div className="flex items-center justify-center min-h-[400px]">
				<div className="flex flex-col items-center space-y-4">
					<Loader2 className="w-8 h-8 animate-spin text-indigo-600" />
					<p className="text-gray-500">Loading sessions...</p>
				</div>
			</div>
		);
	}

	if (!sessions || sessions.length === 0) {
		return (
			<div className="space-y-6">
				<div className="flex items-center justify-between">
					<div className="flex items-center space-x-3">
						<Activity className="w-6 h-6 text-indigo-600" />
						<h1 className="text-2xl font-bold text-gray-900">Network Analyzer</h1>
					</div>
					<button
						onClick={() => setShowStartModal(true)}
						className="flex items-center space-x-2 px-4 py-2 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 transition-colors"
					>
						<Play className="w-4 h-4" />
						<span className="text-sm font-medium">Start Test</span>
					</button>
				</div>

				<div className="bg-white rounded-lg border border-gray-200 p-12 text-center">
					<Activity className="w-12 h-12 text-gray-300 mx-auto mb-4" />
					<h2 className="text-xl font-semibold text-gray-900 mb-2">
						No Test Sessions Yet
					</h2>
					<p className="text-gray-500 mb-6 max-w-md mx-auto">
						Run a network analysis test to measure performance across your fleet.
					</p>
					<button
						onClick={() => setShowStartModal(true)}
						className="inline-flex items-center space-x-2 px-6 py-3 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 transition-colors"
					>
						<Play className="w-5 h-5" />
						<span className="font-medium">Run First Test</span>
					</button>
				</div>

				<StartTestModal
					isOpen={showStartModal}
					onClose={() => setShowStartModal(false)}
					onStart={handleStartTest}
					isPending={startTestMutation.isPending}
					isError={startTestMutation.isError}
					durationMinutes={durationMinutes}
					onDurationChange={setDurationMinutes}
					labelFilter={labelFilter}
					onLabelFilterChange={setLabelFilter}
				/>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			{/* Header */}
			<div className="flex items-center justify-between">
				<div className="flex items-center space-x-3">
					<Activity className="w-6 h-6 text-indigo-600" />
					<h1 className="text-2xl font-bold text-gray-900">Network Analyzer</h1>
				</div>

				<div className="flex items-center space-x-3">
					{/* Start Test Button */}
					<button
						onClick={() => setShowStartModal(true)}
						disabled={isTestRunning}
						className={`flex items-center space-x-2 px-4 py-2 rounded-lg transition-colors ${
							isTestRunning
								? "bg-gray-100 text-gray-400 cursor-not-allowed"
								: "bg-indigo-600 text-white hover:bg-indigo-700"
						}`}
					>
						<Play className="w-4 h-4" />
						<span className="text-sm font-medium">Start Test</span>
					</button>

					{/* Refresh Button */}
					<button
						onClick={() => {
							refetchSessions();
							if (selectedSessionId) refetchStatus();
						}}
						className="p-2 rounded-md border border-gray-200 hover:bg-gray-50 transition-colors"
						title="Refresh data"
					>
						<RefreshCw
							className={`w-4 h-4 text-gray-500 ${statusLoading ? "animate-spin" : ""}`}
						/>
					</button>

					{/* Session Selector */}
					<div className="relative">
						<button
							onClick={() => setShowSessionDropdown(!showSessionDropdown)}
							className="flex items-center space-x-2 px-4 py-2 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors"
						>
							<Calendar className="w-4 h-4 text-gray-500" />
							<span className="text-sm font-medium text-gray-700">
								{selectedSession
									? formatSessionDate(selectedSession.created_at)
									: "Select session"}
							</span>
							<span className="text-xs text-gray-400">
								{selectedSession && `${selectedSession.device_count} devices`}
							</span>
							<ChevronDown
								className={`w-4 h-4 text-gray-400 transition-transform ${
									showSessionDropdown ? "rotate-180" : ""
								}`}
							/>
						</button>

						{showSessionDropdown && (
							<div className="absolute right-0 mt-1 w-72 bg-white border border-gray-200 rounded-lg shadow-lg z-50">
								<div className="max-h-64 overflow-y-auto">
									{sessions.map((session) => (
										<button
											key={session.session_id}
											onClick={() => handleSessionSelect(session.session_id)}
											className={`w-full px-4 py-3 text-left hover:bg-gray-50 transition-colors flex items-center justify-between ${
												selectedSessionId === session.session_id
													? "bg-indigo-50"
													: ""
											}`}
										>
											<div>
												<div className="text-sm font-medium text-gray-900">
													{formatSessionDate(session.created_at)}
												</div>
												<div className="text-xs text-gray-500">
													{session.device_count} devices ·{" "}
													{session.completed_count} completed
												</div>
											</div>
											<span
												className={`px-2 py-0.5 text-xs rounded-full ${
													session.status === "completed"
														? "bg-green-100 text-green-800"
														: session.status === "canceled"
															? "bg-amber-100 text-amber-800"
															: session.status === "running" ||
																  session.status === "partial"
																? "bg-blue-100 text-blue-800"
																: "bg-gray-100 text-gray-800"
												}`}
											>
												{session.status}
											</span>
										</button>
									))}
								</div>
							</div>
						)}
					</div>
				</div>
			</div>

			{/* Content */}
			{statusLoading ? (
				<div className="flex items-center justify-center min-h-[400px]">
					<div className="flex flex-col items-center space-y-4">
						<Loader2 className="w-8 h-8 animate-spin text-indigo-600" />
						<p className="text-gray-500">Loading session data...</p>
					</div>
				</div>
			) : sessionStatus ? (
				<div className="space-y-6">
					{/* Test Overview */}
					<DepartmentOverview data={sessionStatus} />

					{/* Insights Cards */}
					<InsightsCards data={sessionStatus} />

					{/* Device Table and Inspector */}
					<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
						<DeviceTable
							data={sessionStatus}
							onSelectDevice={setSelectedDevice}
							selectedDeviceId={selectedDevice?.device_id ?? null}
						/>

						{selectedDevice ? (
							<DeviceInspector
								device={selectedDevice}
								onClose={() => setSelectedDevice(null)}
							/>
						) : (
							<div className="bg-white rounded-lg border border-gray-200 p-8 flex items-center justify-center min-h-[400px]">
								<div className="text-center">
									<Activity className="w-10 h-10 text-gray-300 mx-auto mb-3" />
									<p className="text-gray-500">
										Select a device to view detailed performance metrics
									</p>
								</div>
							</div>
						)}
					</div>
				</div>
			) : (
				<div className="bg-white rounded-lg border border-gray-200 p-12 text-center">
					<p className="text-gray-500">Select a session to view results</p>
				</div>
			)}

			<StartTestModal
				isOpen={showStartModal}
				onClose={() => setShowStartModal(false)}
				onStart={handleStartTest}
				isPending={startTestMutation.isPending}
				isError={startTestMutation.isError}
				durationMinutes={durationMinutes}
				onDurationChange={setDurationMinutes}
				labelFilter={labelFilter}
				onLabelFilterChange={setLabelFilter}
			/>
		</div>
	);
}
