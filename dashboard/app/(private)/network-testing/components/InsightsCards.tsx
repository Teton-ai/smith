"use client";

import { Activity, Gauge, Wifi } from "lucide-react";
import type {
	AggregateEvaluation,
	ExtendedTestStatus,
} from "../hooks/useExtendedTest";

interface InsightsCardsProps {
	data: ExtendedTestStatus;
}

function getBandwidthHealthStyle(label: string): {
	color: string;
	bgColor: string;
	borderColor: string;
} {
	if (label === "Stable") {
		return {
			color: "text-green-700",
			bgColor: "bg-green-50",
			borderColor: "border-green-200",
		};
	}
	if (label === "Moderate Degradation") {
		return {
			color: "text-yellow-700",
			bgColor: "bg-yellow-50",
			borderColor: "border-yellow-200",
		};
	}
	return {
		color: "text-red-700",
		bgColor: "bg-red-50",
		borderColor: "border-red-200",
	};
}

function getSpeedTierStyle(tier: string): {
	color: string;
	bgColor: string;
	borderColor: string;
} {
	if (tier === "Fast") {
		return {
			color: "text-green-700",
			bgColor: "bg-green-50",
			borderColor: "border-green-200",
		};
	}
	if (tier === "Moderate") {
		return {
			color: "text-blue-700",
			bgColor: "bg-blue-50",
			borderColor: "border-blue-200",
		};
	}
	return {
		color: "text-red-700",
		bgColor: "bg-red-50",
		borderColor: "border-red-200",
	};
}

function getCoverageQualityStyle(quality: string): {
	color: string;
	bgColor: string;
	borderColor: string;
} {
	if (quality === "Consistent") {
		return {
			color: "text-green-700",
			bgColor: "bg-green-50",
			borderColor: "border-green-200",
		};
	}
	if (quality === "Variable") {
		return {
			color: "text-yellow-700",
			bgColor: "bg-yellow-50",
			borderColor: "border-yellow-200",
		};
	}
	return {
		color: "text-red-700",
		bgColor: "bg-red-50",
		borderColor: "border-red-200",
	};
}

function hasCompletedResults(data: ExtendedTestStatus): boolean {
	return data.results.some(
		(r) =>
			r.status === "completed" && r.minute_stats && r.minute_stats.length > 0,
	);
}

export default function InsightsCards({ data }: InsightsCardsProps) {
	if (!hasCompletedResults(data)) {
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

	const agg: AggregateEvaluation = data.evaluation.aggregate;
	const bandwidthStyle = getBandwidthHealthStyle(agg.bandwidth_health);
	const speedStyle = getSpeedTierStyle(agg.speed_tier);
	const coverageStyle = getCoverageQualityStyle(agg.coverage_quality);

	const bandwidthDescription =
		agg.bandwidth_health === "Stable"
			? "Network performance remained consistent throughout the test"
			: agg.bandwidth_health === "Moderate Degradation"
				? "Bandwidth decreased slightly under sustained load"
				: "Significant speed reduction detected - possible contention issue";

	const speedDescription =
		agg.speed_tier === "Fast"
			? "Excellent speeds suitable for all workloads"
			: agg.speed_tier === "Moderate"
				? "Good speeds for most applications"
				: "May experience issues with bandwidth-intensive tasks";

	const coverageDescription =
		agg.coverage_quality === "Consistent"
			? "All devices experiencing similar network quality"
			: agg.coverage_quality === "Variable"
				? "Some variation in speeds across devices"
				: "High variance suggests WiFi dead zones or mixed connection types";

	return (
		<div className="grid grid-cols-1 md:grid-cols-3 gap-4">
			{/* Bandwidth Health Card */}
			<div
				className={`rounded-lg border p-4 ${bandwidthStyle.bgColor} ${bandwidthStyle.borderColor}`}
			>
				<div className="flex items-center space-x-2 mb-2">
					<Activity className={`w-5 h-5 ${bandwidthStyle.color}`} />
					<span className="text-sm font-medium text-gray-600">
						Bandwidth Health
					</span>
				</div>
				<div className={`text-lg font-bold ${bandwidthStyle.color} mb-1`}>
					{agg.bandwidth_health}
				</div>
				<p className="text-sm text-gray-600">{bandwidthDescription}</p>
				<div className="mt-2 text-xs text-gray-500">
					{agg.bandwidth_health_trend_percent >= 0 ? "+" : ""}
					{agg.bandwidth_health_trend_percent.toFixed(1)}% change over test
					duration
				</div>
			</div>

			{/* Speed Tier Card */}
			<div
				className={`rounded-lg border p-4 ${speedStyle.bgColor} ${speedStyle.borderColor}`}
			>
				<div className="flex items-center space-x-2 mb-2">
					<Gauge className={`w-5 h-5 ${speedStyle.color}`} />
					<span className="text-sm font-medium text-gray-600">Speed Tier</span>
				</div>
				<div className={`text-lg font-bold ${speedStyle.color} mb-1`}>
					{agg.speed_tier} ({agg.average_download_mbps.toFixed(0)} Mbps)
				</div>
				<p className="text-sm text-gray-600">{speedDescription}</p>
			</div>

			{/* Coverage Quality Card */}
			<div
				className={`rounded-lg border p-4 ${coverageStyle.bgColor} ${coverageStyle.borderColor}`}
			>
				<div className="flex items-center space-x-2 mb-2">
					<Wifi className={`w-5 h-5 ${coverageStyle.color}`} />
					<span className="text-sm font-medium text-gray-600">
						Coverage Quality
					</span>
				</div>
				<div className={`text-lg font-bold ${coverageStyle.color} mb-1`}>
					{agg.coverage_quality}
				</div>
				<p className="text-sm text-gray-600">{coverageDescription}</p>
				<div className="mt-2 text-xs text-gray-500">
					Coefficient of variation: {agg.coefficient_of_variation.toFixed(1)}%
				</div>
			</div>
		</div>
	);
}
