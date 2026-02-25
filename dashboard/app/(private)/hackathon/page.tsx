"use client";

import { Activity, ArrowLeft, Loader2, Play } from "lucide-react";
import { useSearchParams, useRouter } from "next/navigation";
import { useCallback, useState } from "react";
import { type Device } from "@/app/api-client";
import DepartmentOverview from "./components/DepartmentOverview";
import DeviceInspector from "./components/DeviceInspector";
import DeviceSelector, { type SelectionMode } from "./components/DeviceSelector";
import DeviceTable from "./components/DeviceTable";
import InsightsCards from "./components/InsightsCards";
import SessionHistory from "./components/SessionHistory";
import StartTestModal from "./components/StartTestModal";
import {
	type DeviceExtendedTestResult,
	type ExtendedTestSessionSummary,
	useStartExtendedTest,
	useExtendedTestStatus,
	useSessionsByDevices,
} from "./hooks/useExtendedTest";

type ViewMode = "landing" | "results";

export default function HackathonPage() {
	const router = useRouter();
	const searchParams = useSearchParams();

	// URL params for deep linking
	const sessionFromUrl = searchParams.get("session");

	// View mode: landing (device selection + history) or results (test details)
	const [viewMode, setViewMode] = useState<ViewMode>(sessionFromUrl ? "results" : "landing");
	const [selectedSessionId, setSelectedSessionId] = useState<string | null>(sessionFromUrl);

	// Device selection state
	const [selectionMode, setSelectionMode] = useState<SelectionMode>("labels");
	const [selectedLabels, setSelectedLabels] = useState<string[]>([]);
	const [selectedDeviceIds, setSelectedDeviceIds] = useState<Set<number>>(new Set());
	const [resolvedDevices, setResolvedDevices] = useState<Device[]>([]);

	// Modal and test state
	const [showStartModal, setShowStartModal] = useState(false);
	const [durationMinutes, setDurationMinutes] = useState(3);
	const [selectedDevice, setSelectedDevice] = useState<DeviceExtendedTestResult | null>(null);

	// Get serial numbers from resolved devices
	const serialNumbers = resolvedDevices.map((d) => d.serial_number);

	// Fetch sessions matching the selected devices
	const {
		data: matchingSessions,
		isLoading: matchingSessionsLoading,
		refetch: refetchMatchingSessions,
	} = useSessionsByDevices(serialNumbers);

	// Fetch selected session status
	const {
		data: sessionStatus,
		isLoading: statusLoading,
	} = useExtendedTestStatus(selectedSessionId);

	const startTestMutation = useStartExtendedTest();

	// Handle devices resolved from selector
	const handleDevicesResolved = useCallback((devices: Device[]) => {
		setResolvedDevices(devices);
	}, []);

	// Handle session selection from history or dropdown
	const handleSessionSelect = (session: ExtendedTestSessionSummary) => {
		setSelectedSessionId(session.session_id);
		setSelectedDevice(null);
		setViewMode("results");
		router.replace(`/hackathon?session=${session.session_id}`);
	};

	// Handle start test
	const handleStartTest = async () => {
		const labelFilter = selectionMode === "labels" ? selectedLabels.join(",") : "";
		try {
			const result = await startTestMutation.mutateAsync({
				label_filter: labelFilter,
				duration_minutes: durationMinutes,
			});
			setShowStartModal(false);
			setSelectedSessionId(result.session_id);
			setViewMode("results");
			router.replace(`/hackathon?session=${result.session_id}`);
			refetchMatchingSessions();
		} catch (error) {
			console.error("Failed to start test:", error);
		}
	};

	// Handle view for selected devices
	const handleViewResults = () => {
		if (matchingSessions && matchingSessions.length > 0) {
			handleSessionSelect(matchingSessions[0]);
		}
	};

	// Handle back to landing
	const handleBackToLanding = () => {
		setViewMode("landing");
		setSelectedSessionId(null);
		setSelectedDevice(null);
		router.replace("/hackathon");
	};

	const isTestRunning =
		sessionStatus?.status === "running" ||
		sessionStatus?.status === "pending" ||
		sessionStatus?.status === "partial";

	const hasDevicesSelected = resolvedDevices.length > 0;
	const hasMatchingSessions = matchingSessions && matchingSessions.length > 0;

	// Landing view - device selection + history
	if (viewMode === "landing") {
		return (
			<div className="space-y-6">
				{/* Header */}
				<div className="flex items-center space-x-3">
					<Activity className="w-6 h-6 text-indigo-600" />
					<h1 className="text-2xl font-bold text-gray-900">Network Analyzer</h1>
				</div>

				<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
					{/* Device Selection Panel */}
					<div className="bg-white rounded-lg border border-gray-200 p-6">
						<h2 className="text-lg font-semibold text-gray-900 mb-4">
							Select Devices
						</h2>
						<DeviceSelector
							mode={selectionMode}
							onModeChange={setSelectionMode}
							selectedLabels={selectedLabels}
							onLabelsChange={setSelectedLabels}
							selectedDeviceIds={selectedDeviceIds}
							onDeviceIdsChange={setSelectedDeviceIds}
							onDevicesResolved={handleDevicesResolved}
						/>

						{/* Action Buttons */}
						{hasDevicesSelected && (
							<div className="mt-6 pt-4 border-t border-gray-200">
								{matchingSessionsLoading ? (
									<div className="flex items-center space-x-2 text-gray-500 text-sm">
										<Loader2 className="w-4 h-4 animate-spin" />
										<span>Checking for previous tests...</span>
									</div>
								) : hasMatchingSessions ? (
									<div className="space-y-3">
										<p className="text-sm text-gray-600">
											Found {matchingSessions.length} previous test
											{matchingSessions.length !== 1 ? "s" : ""} for these devices
										</p>
										<div className="flex space-x-3">
											<button
												onClick={handleViewResults}
												className="flex-1 flex items-center justify-center space-x-2 px-4 py-2 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 transition-colors"
											>
												<Activity className="w-4 h-4" />
												<span className="text-sm font-medium">
													View Latest Results
												</span>
											</button>
											<button
												onClick={() => setShowStartModal(true)}
												className="flex items-center justify-center space-x-2 px-4 py-2 bg-white text-gray-700 border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors"
											>
												<Play className="w-4 h-4" />
												<span className="text-sm font-medium">New Test</span>
											</button>
										</div>
									</div>
								) : (
									<div className="space-y-3">
										<p className="text-sm text-gray-600">
											No previous tests found for these {resolvedDevices.length} devices
										</p>
										<button
											onClick={() => setShowStartModal(true)}
											className="w-full flex items-center justify-center space-x-2 px-4 py-3 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 transition-colors"
										>
											<Play className="w-5 h-5" />
											<span className="font-medium">Start Network Test</span>
										</button>
									</div>
								)}
							</div>
						)}
					</div>

					{/* History Panel */}
					<div className="bg-white rounded-lg border border-gray-200 p-6">
						<h2 className="text-lg font-semibold text-gray-900 mb-4">
							Test History
						</h2>
						<SessionHistory onSelectSession={handleSessionSelect} />
					</div>
				</div>

				<StartTestModal
					isOpen={showStartModal}
					onClose={() => setShowStartModal(false)}
					onStart={handleStartTest}
					isPending={startTestMutation.isPending}
					isError={startTestMutation.isError}
					durationMinutes={durationMinutes}
					onDurationChange={setDurationMinutes}
					selectedLabels={selectedLabels}
					selectedDeviceCount={resolvedDevices.length}
					selectionMode={selectionMode}
				/>
			</div>
		);
	}

	// Results view - show test details
	return (
		<div className="space-y-6">
			{/* Header */}
			<div className="flex items-center space-x-3">
				<button
					onClick={handleBackToLanding}
					className="p-1 rounded-md hover:bg-gray-100 transition-colors"
				>
					<ArrowLeft className="w-5 h-5 text-gray-500" />
				</button>
				<Activity className="w-6 h-6 text-indigo-600" />
				<h1 className="text-2xl font-bold text-gray-900">Network Analyzer</h1>
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
		</div>
	);
}
