import { Button, SearchInput } from "@teton/smith-ui";
import { Activity, ArrowLeft, Loader2, Plus } from "lucide-react";
import { useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import DepartmentOverview from "./components/DepartmentOverview";
import DeviceInspector from "./components/DeviceInspector";
import DeviceTable from "./components/DeviceTable";
import InsightsCards from "./components/InsightsCards";
import SessionHistory from "./components/SessionHistory";
import StartTestModal from "./components/StartTestModal";
import {
	type ExtendedTestSessionSummary,
	useExtendedTestStatus,
} from "./hooks/useExtendedTest";

type ViewMode = "landing" | "results";

export default function NetworkTestingPage() {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();

	// URL params for deep linking
	const sessionFromUrl = searchParams.get("session");

	// View mode: landing (history) or results (test details)
	const [viewMode, setViewMode] = useState<ViewMode>(
		sessionFromUrl ? "results" : "landing",
	);
	const [selectedSessionId, setSelectedSessionId] = useState<string | null>(
		sessionFromUrl,
	);

	const [showStartModal, setShowStartModal] = useState(false);
	const [selectedDeviceId, setSelectedDeviceId] = useState<number | null>(null);
	const [searchQuery, setSearchQuery] = useState("");

	// Fetch selected session status
	const { data: sessionStatus, isLoading: statusLoading } =
		useExtendedTestStatus(selectedSessionId);

	// Derive selected device from live session data to avoid stale object during polling
	const selectedDevice =
		sessionStatus?.results.find((d) => d.device_id === selectedDeviceId) ??
		null;

	// Handle session selection from history
	const handleSessionSelect = (session: ExtendedTestSessionSummary) => {
		setSelectedSessionId(session.session_id);
		setSelectedDeviceId(null);
		setViewMode("results");
		navigate(`/network-testing?session=${session.session_id}`, {
			replace: true,
		});
	};

	// Handle a freshly started test
	const handleTestStarted = (sessionId: string) => {
		setShowStartModal(false);
		setSelectedSessionId(sessionId);
		setSelectedDeviceId(null);
		setViewMode("results");
		navigate(`/network-testing?session=${sessionId}`, { replace: true });
	};

	// Handle back to landing
	const handleBackToLanding = () => {
		setViewMode("landing");
		setSelectedSessionId(null);
		setSelectedDeviceId(null);
		navigate("/network-testing", { replace: true });
	};

	// Landing view - test history
	if (viewMode === "landing") {
		return (
			<div className="flex-1 overflow-y-auto px-4 sm:px-6 lg:px-8 py-6 space-y-4">
				{/* Toolbar: search + new test */}
				<div className="flex items-center justify-between gap-3">
					<SearchInput
						value={searchQuery}
						onChange={setSearchQuery}
						placeholder="Search by label, date, or status..."
						className="max-w-md"
					/>
					<Button
						variant="solid"
						tone="blue"
						icon={<Plus className="w-4 h-4" />}
						onClick={() => setShowStartModal(true)}
					>
						New Test
					</Button>
				</div>

				<SessionHistory
					searchQuery={searchQuery}
					onSelectSession={handleSessionSelect}
				/>

				<StartTestModal
					isOpen={showStartModal}
					onClose={() => setShowStartModal(false)}
					onStarted={handleTestStarted}
				/>
			</div>
		);
	}

	// Results view - show test details
	return (
		<div className="flex-1 overflow-y-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
			{/* Header */}
			<div className="flex items-center space-x-3">
				<button
					onClick={handleBackToLanding}
					aria-label="Back to network test history"
					className="p-1 rounded-md hover:bg-gray-100 transition-colors"
				>
					<ArrowLeft className="w-5 h-5 text-gray-500" />
				</button>
				<Activity className="w-6 h-6 text-blue-600" />
				<h1 className="text-2xl font-bold text-gray-900">Network Analyzer</h1>
			</div>

			{/* Content */}
			{statusLoading ? (
				<div className="flex items-center justify-center min-h-[400px]">
					<div className="flex flex-col items-center space-y-4">
						<Loader2 className="w-8 h-8 animate-spin text-blue-600" />
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
							onSelectDevice={(device) => setSelectedDeviceId(device.device_id)}
							selectedDeviceId={selectedDeviceId}
						/>

						{selectedDevice ? (
							<DeviceInspector
								device={selectedDevice}
								evaluation={sessionStatus.evaluation.per_device.find(
									(e) => e.device_id === selectedDevice.device_id,
								)}
								onClose={() => setSelectedDeviceId(null)}
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
