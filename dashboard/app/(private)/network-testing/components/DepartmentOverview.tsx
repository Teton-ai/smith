"use client";

import { Activity, CheckCircle, Clock, Cpu, Loader2, StopCircle, TrendingDown, TrendingUp } from "lucide-react";
import { useEffect, useState } from "react";
import { type ExtendedTestStatus, useCancelExtendedTest } from "../hooks/useExtendedTest";

interface DepartmentOverviewProps {
	data: ExtendedTestStatus;
}

function calculateAggregateStats(data: ExtendedTestStatus) {
	const completedResults = data.results.filter(
		(r) => r.status === "completed" && r.minute_stats && r.minute_stats.length > 0
	);

	if (completedResults.length === 0) {
		return null;
	}

	// Aggregate download speeds across all devices
	const allDownloadSpeeds: number[] = [];
	const firstMinuteSpeeds: number[] = [];
	const lastMinuteSpeeds: number[] = [];

	for (const result of completedResults) {
		if (!result.minute_stats) continue;

		for (const stat of result.minute_stats) {
			allDownloadSpeeds.push(stat.download.average_mbps);
		}

		// First and last minute for trend calculation
		const sorted = [...result.minute_stats].sort((a, b) => a.minute - b.minute);
		if (sorted.length > 0) {
			firstMinuteSpeeds.push(sorted[0].download.average_mbps);
			lastMinuteSpeeds.push(sorted[sorted.length - 1].download.average_mbps);
		}
	}

	const avgDownload =
		allDownloadSpeeds.reduce((a, b) => a + b, 0) / allDownloadSpeeds.length;
	const minDownload = Math.min(...allDownloadSpeeds);
	const maxDownload = Math.max(...allDownloadSpeeds);

	// Calculate standard deviation
	const variance =
		allDownloadSpeeds.reduce((sum, val) => sum + Math.pow(val - avgDownload, 2), 0) /
		allDownloadSpeeds.length;
	const stdDev = Math.sqrt(variance);

	// Calculate bandwidth trend (first minute vs last minute)
	const avgFirstMinute =
		firstMinuteSpeeds.reduce((a, b) => a + b, 0) / firstMinuteSpeeds.length;
	const avgLastMinute =
		lastMinuteSpeeds.reduce((a, b) => a + b, 0) / lastMinuteSpeeds.length;
	const trendPercent =
		avgFirstMinute > 0
			? ((avgLastMinute - avgFirstMinute) / avgFirstMinute) * 100
			: 0;

	return {
		avgDownload,
		minDownload,
		maxDownload,
		stdDev,
		trendPercent,
		deviceCount: completedResults.length,
	};
}

function getBandwidthHealth(trendPercent: number): {
	label: string;
	color: string;
	bgColor: string;
} {
	if (trendPercent >= -10) {
		return { label: "Stable", color: "text-green-800", bgColor: "bg-green-100" };
	}
	if (trendPercent >= -25) {
		return { label: "Moderate Degradation", color: "text-yellow-800", bgColor: "bg-yellow-100" };
	}
	return { label: "Severe Degradation", color: "text-red-800", bgColor: "bg-red-100" };
}

function calculateTimeRemaining(createdAt: string, durationMinutes: number): { remaining: number; elapsed: number; progress: number } {
	const startTime = new Date(createdAt).getTime();
	const endTime = startTime + durationMinutes * 60 * 1000;
	const now = Date.now();
	const elapsed = Math.floor((now - startTime) / 1000);
	// Allow negative remaining to calculate overtime
	const remaining = Math.floor((endTime - now) / 1000);
	const progress = Math.min(100, (elapsed / (durationMinutes * 60)) * 100);
	return { remaining, elapsed, progress };
}

function formatSeconds(seconds: number): string {
	const mins = Math.floor(seconds / 60);
	const secs = seconds % 60;
	return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export default function DepartmentOverview({ data }: DepartmentOverviewProps) {
	const [, setTick] = useState(0);
	const cancelMutation = useCancelExtendedTest();
	const stats = calculateAggregateStats(data);
	const completionRate =
		data.device_count > 0
			? Math.round((data.completed_count / data.device_count) * 100)
			: 0;

	const bandwidthHealth = stats ? getBandwidthHealth(stats.trendPercent) : null;
	const isRunning = data.status === "running" || data.status === "pending" || data.status === "partial";
	const timeInfo = isRunning ? calculateTimeRemaining(data.created_at, data.duration_minutes) : null;

	// Overtime = timer expired AND not all devices completed
	const allDevicesCompleted = data.completed_count >= data.device_count;
	const timerExpired = timeInfo && timeInfo.remaining < 0;
	const overtimeSeconds = timerExpired ? Math.abs(timeInfo.remaining) : 0;
	// Only show amber warning if overtime > 2 minutes
	const isOvertime = timerExpired && !allDevicesCompleted && overtimeSeconds > 120;

	// Update countdown every second while test is running
	useEffect(() => {
		if (!isRunning) return;
		const interval = setInterval(() => setTick((t) => t + 1), 1000);
		return () => clearInterval(interval);
	}, [isRunning]);

	const handleSuspend = () => {
		cancelMutation.mutate(data.session_id);
	};

	return (
		<div className="bg-white rounded-lg border border-gray-200 p-6">
			{/* Progress Banner for Running Tests */}
			{isRunning && timeInfo && (
				<div className={`mb-6 rounded-lg p-4 ${isOvertime ? "bg-amber-50 border border-amber-200" : "bg-blue-50 border border-blue-200"}`}>
					<div className="flex items-center justify-between mb-2">
						<div className="flex items-center space-x-2">
							<Loader2 className={`w-5 h-5 animate-spin ${isOvertime ? "text-amber-600" : "text-blue-600"}`} />
							<span className={`font-medium ${isOvertime ? "text-amber-900" : "text-blue-900"}`}>
								{isOvertime ? "Test Running Over Time" : "Test in Progress"}
							</span>
						</div>
						<div className="flex items-center space-x-3">
							{timerExpired ? (
								<span className={`text-sm ${isOvertime ? "text-amber-700" : "text-blue-700"}`}>
									+{formatSeconds(overtimeSeconds)} overtime
								</span>
							) : (
								<span className="text-sm text-blue-700">
									{formatSeconds(Math.max(0, timeInfo.remaining))} remaining
								</span>
							)}
							<button
								onClick={handleSuspend}
								disabled={cancelMutation.isPending}
								className={`flex items-center space-x-1 px-3 py-1 text-sm font-medium rounded-md transition-colors ${
									isOvertime
										? "bg-amber-600 text-white hover:bg-amber-700"
										: "bg-gray-200 text-gray-700 hover:bg-gray-300"
								} disabled:opacity-50`}
							>
								{cancelMutation.isPending ? (
									<Loader2 className="w-4 h-4 animate-spin" />
								) : (
									<StopCircle className="w-4 h-4" />
								)}
								<span>{timerExpired ? "Finish Now" : "Suspend"}</span>
							</button>
						</div>
					</div>
					<div className={`w-full rounded-full h-2 ${isOvertime ? "bg-amber-200" : "bg-blue-200"}`}>
						<div
							className={`h-2 rounded-full transition-all duration-1000 ${isOvertime ? "bg-amber-600" : "bg-blue-600"}`}
							style={{ width: `${Math.min(100, timeInfo.progress)}%` }}
						/>
					</div>
					<div className={`flex items-center justify-between mt-2 text-xs ${isOvertime ? "text-amber-600" : "text-blue-600"}`}>
						<span>{data.completed_count} of {data.device_count} devices completed</span>
						<span>{Math.round(Math.min(100, timeInfo.progress))}% complete</span>
					</div>
				</div>
			)}

			<div className="flex items-center justify-between mb-6">
				<h2 className="text-lg font-semibold text-gray-900">Test Overview</h2>
				<div className="flex items-center space-x-2">
					<span
						className={`px-2.5 py-1 rounded-full text-xs font-medium ${
							data.status === "completed"
								? "bg-green-100 text-green-800"
								: data.status === "canceled"
									? "bg-amber-100 text-amber-800"
									: data.status === "running" || data.status === "partial"
										? "bg-blue-100 text-blue-800"
										: "bg-gray-100 text-gray-800"
						}`}
					>
						{data.status.charAt(0).toUpperCase() + data.status.slice(1)}
					</span>
				</div>
			</div>

			<div className="grid grid-cols-2 md:grid-cols-4 gap-6">
				{/* Devices Tested */}
				<div className="space-y-1">
					<div className="flex items-center space-x-2 text-gray-500 text-sm">
						<Cpu className="w-4 h-4" />
						<span>Devices Tested</span>
					</div>
					<div className="text-2xl font-bold text-gray-900">{data.device_count}</div>
					<div className="flex items-center space-x-1 text-sm">
						<CheckCircle className="w-4 h-4 text-green-500" />
						<span className="text-gray-600">{completionRate}% completed</span>
					</div>
				</div>

				{/* Test Duration */}
				<div className="space-y-1">
					<div className="flex items-center space-x-2 text-gray-500 text-sm">
						<Clock className="w-4 h-4" />
						<span>Test Duration</span>
					</div>
					<div className="text-2xl font-bold text-gray-900">
						{data.duration_minutes} min
					</div>
					<div className="text-sm text-gray-600">
						Started {new Date(data.created_at).toLocaleTimeString()}
					</div>
				</div>

				{/* Avg Download Speed */}
				<div className="space-y-1">
					<div className="flex items-center space-x-2 text-gray-500 text-sm">
						<Activity className="w-4 h-4" />
						<span>Avg Download</span>
					</div>
					{stats ? (
						<>
							<div className="text-2xl font-bold text-gray-900">
								{stats.avgDownload.toFixed(1)} <span className="text-sm font-normal">Mbps</span>
							</div>
							<div className="text-sm text-gray-600">
								Range: {stats.minDownload.toFixed(1)} - {stats.maxDownload.toFixed(1)} Mbps
							</div>
						</>
					) : (
						<div className="text-2xl font-bold text-gray-400">--</div>
					)}
				</div>

				{/* Bandwidth Health */}
				<div className="space-y-1">
					<div className="flex items-center space-x-2 text-gray-500 text-sm">
						{stats && stats.trendPercent < 0 ? (
							<TrendingDown className="w-4 h-4" />
						) : (
							<TrendingUp className="w-4 h-4" />
						)}
						<span>Bandwidth Health</span>
					</div>
					{stats && bandwidthHealth ? (
						<>
							<div className="text-2xl font-bold text-gray-900">
								<span
									className={`px-2 py-0.5 rounded text-sm font-medium ${bandwidthHealth.bgColor} ${bandwidthHealth.color}`}
								>
									{bandwidthHealth.label}
								</span>
							</div>
							<div className="flex items-center text-sm text-gray-600">
								{stats.trendPercent < 0 ? (
									<TrendingDown className="w-4 h-4 mr-1 text-red-500" />
								) : (
									<TrendingUp className="w-4 h-4 mr-1 text-green-500" />
								)}
								{Math.abs(stats.trendPercent).toFixed(1)}% over test
							</div>
						</>
					) : (
						<div className="text-2xl font-bold text-gray-400">--</div>
					)}
				</div>
			</div>
		</div>
	);
}
