"use client";

import { Activity, Gauge, Wifi } from "lucide-react";
import type { ExtendedTestStatus } from "../hooks/useExtendedTest";

interface InsightsCardsProps {
	data: ExtendedTestStatus;
}

function calculateInsights(data: ExtendedTestStatus) {
	const completedResults = data.results.filter(
		(r) => r.status === "completed" && r.minute_stats && r.minute_stats.length > 0
	);

	if (completedResults.length === 0) {
		return null;
	}

	// Collect all average speeds per device
	const deviceAvgSpeeds: number[] = [];
	const deviceVariances: number[] = [];
	const firstMinuteSpeeds: number[] = [];
	const lastMinuteSpeeds: number[] = [];

	for (const result of completedResults) {
		if (!result.minute_stats || result.minute_stats.length === 0) continue;

		// Calculate per-device average
		const speeds = result.minute_stats.map((s) => s.download.average_mbps);
		const avg = speeds.reduce((a, b) => a + b, 0) / speeds.length;
		deviceAvgSpeeds.push(avg);

		// Calculate per-device variance (consistency)
		const variance =
			speeds.reduce((sum, val) => sum + Math.pow(val - avg, 2), 0) / speeds.length;
		deviceVariances.push(variance);

		// First and last minute for trend
		const sorted = [...result.minute_stats].sort((a, b) => a.minute - b.minute);
		firstMinuteSpeeds.push(sorted[0].download.average_mbps);
		lastMinuteSpeeds.push(sorted[sorted.length - 1].download.average_mbps);
	}

	// Overall average speed
	const overallAvg =
		deviceAvgSpeeds.reduce((a, b) => a + b, 0) / deviceAvgSpeeds.length;

	// Overall variance (consistency across devices)
	const overallVariance =
		deviceAvgSpeeds.reduce((sum, val) => sum + Math.pow(val - overallAvg, 2), 0) /
		deviceAvgSpeeds.length;
	const coefficientOfVariation = (Math.sqrt(overallVariance) / overallAvg) * 100;

	// Bandwidth trend
	const avgFirstMinute =
		firstMinuteSpeeds.reduce((a, b) => a + b, 0) / firstMinuteSpeeds.length;
	const avgLastMinute =
		lastMinuteSpeeds.reduce((a, b) => a + b, 0) / lastMinuteSpeeds.length;
	const trendPercent =
		avgFirstMinute > 0
			? ((avgLastMinute - avgFirstMinute) / avgFirstMinute) * 100
			: 0;

	return {
		overallAvg,
		coefficientOfVariation,
		trendPercent,
	};
}

function getBandwidthHealthInsight(trendPercent: number): {
	title: string;
	description: string;
	color: string;
	bgColor: string;
	borderColor: string;
} {
	if (trendPercent >= -10) {
		return {
			title: "Bandwidth Stable",
			description: "Network performance remained consistent throughout the test",
			color: "text-green-700",
			bgColor: "bg-green-50",
			borderColor: "border-green-200",
		};
	}
	if (trendPercent >= -25) {
		return {
			title: "Moderate Degradation",
			description: "Bandwidth decreased slightly under sustained load",
			color: "text-yellow-700",
			bgColor: "bg-yellow-50",
			borderColor: "border-yellow-200",
		};
	}
	return {
		title: "Bandwidth Degrades Under Load",
		description: "Significant speed reduction detected - possible contention issue",
		color: "text-red-700",
		bgColor: "bg-red-50",
		borderColor: "border-red-200",
	};
}

function getSpeedTierInsight(avgMbps: number): {
	tier: string;
	description: string;
	color: string;
	bgColor: string;
	borderColor: string;
} {
	if (avgMbps >= 100) {
		return {
			tier: "Fast",
			description: "Excellent speeds suitable for all workloads",
			color: "text-green-700",
			bgColor: "bg-green-50",
			borderColor: "border-green-200",
		};
	}
	if (avgMbps >= 50) {
		return {
			tier: "Moderate",
			description: "Good speeds for most applications",
			color: "text-blue-700",
			bgColor: "bg-blue-50",
			borderColor: "border-blue-200",
		};
	}
	return {
		tier: "Slow",
		description: "May experience issues with bandwidth-intensive tasks",
		color: "text-red-700",
		bgColor: "bg-red-50",
		borderColor: "border-red-200",
	};
}

function getCoverageQualityInsight(cv: number): {
	quality: string;
	description: string;
	color: string;
	bgColor: string;
	borderColor: string;
} {
	if (cv <= 20) {
		return {
			quality: "Consistent",
			description: "All devices experiencing similar network quality",
			color: "text-green-700",
			bgColor: "bg-green-50",
			borderColor: "border-green-200",
		};
	}
	if (cv <= 40) {
		return {
			quality: "Variable",
			description: "Some variation in speeds across devices",
			color: "text-yellow-700",
			bgColor: "bg-yellow-50",
			borderColor: "border-yellow-200",
		};
	}
	return {
		quality: "Poor Coverage",
		description: "High variance suggests WiFi dead zones or mixed connection types",
		color: "text-red-700",
		bgColor: "bg-red-50",
		borderColor: "border-red-200",
	};
}

export default function InsightsCards({ data }: InsightsCardsProps) {
	const insights = calculateInsights(data);

	if (!insights) {
		return (
			<div className="grid grid-cols-1 md:grid-cols-3 gap-4">
				{[1, 2, 3].map((i) => (
					<div
						key={i}
						className="bg-gray-50 rounded-lg border border-gray-200 p-4 animate-pulse"
					>
						<div className="h-4 bg-gray-200 rounded w-1/3 mb-2" />
						<div className="h-6 bg-gray-200 rounded w-2/3 mb-2" />
						<div className="h-3 bg-gray-200 rounded w-full" />
					</div>
				))}
			</div>
		);
	}

	const bandwidthHealth = getBandwidthHealthInsight(insights.trendPercent);
	const speedTier = getSpeedTierInsight(insights.overallAvg);
	const coverageQuality = getCoverageQualityInsight(insights.coefficientOfVariation);

	return (
		<div className="grid grid-cols-1 md:grid-cols-3 gap-4">
			{/* Bandwidth Health Card */}
			<div
				className={`rounded-lg border p-4 ${bandwidthHealth.bgColor} ${bandwidthHealth.borderColor}`}
			>
				<div className="flex items-center space-x-2 mb-2">
					<Activity className={`w-5 h-5 ${bandwidthHealth.color}`} />
					<span className="text-sm font-medium text-gray-600">Bandwidth Health</span>
				</div>
				<div className={`text-lg font-bold ${bandwidthHealth.color} mb-1`}>
					{bandwidthHealth.title}
				</div>
				<p className="text-sm text-gray-600">{bandwidthHealth.description}</p>
				<div className="mt-2 text-xs text-gray-500">
					{insights.trendPercent >= 0 ? "+" : ""}
					{insights.trendPercent.toFixed(1)}% change over test duration
				</div>
			</div>

			{/* Speed Tier Card */}
			<div
				className={`rounded-lg border p-4 ${speedTier.bgColor} ${speedTier.borderColor}`}
			>
				<div className="flex items-center space-x-2 mb-2">
					<Gauge className={`w-5 h-5 ${speedTier.color}`} />
					<span className="text-sm font-medium text-gray-600">Speed Tier</span>
				</div>
				<div className={`text-lg font-bold ${speedTier.color} mb-1`}>
					{speedTier.tier} ({insights.overallAvg.toFixed(0)} Mbps)
				</div>
				<p className="text-sm text-gray-600">{speedTier.description}</p>
			</div>

			{/* Coverage Quality Card */}
			<div
				className={`rounded-lg border p-4 ${coverageQuality.bgColor} ${coverageQuality.borderColor}`}
			>
				<div className="flex items-center space-x-2 mb-2">
					<Wifi className={`w-5 h-5 ${coverageQuality.color}`} />
					<span className="text-sm font-medium text-gray-600">Coverage Quality</span>
				</div>
				<div className={`text-lg font-bold ${coverageQuality.color} mb-1`}>
					{coverageQuality.quality}
				</div>
				<p className="text-sm text-gray-600">{coverageQuality.description}</p>
				<div className="mt-2 text-xs text-gray-500">
					Coefficient of variation: {insights.coefficientOfVariation.toFixed(1)}%
				</div>
			</div>
		</div>
	);
}
