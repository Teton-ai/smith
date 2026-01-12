"use client";

import { WifiOff } from "lucide-react";
import type React from "react";

interface NetworkQualityIndicatorProps {
	isOnline: boolean;
	networkScore?: number;
	className?: string;
}

const NetworkQualityIndicator: React.FC<NetworkQualityIndicatorProps> = ({
	isOnline,
	networkScore,
	className = "",
}) => {
	// Offline - show disconnected icon
	if (!isOnline) {
		return <WifiOff className={`w-4 h-4 text-red-500 ${className}`} />;
	}

	// Online but no network score - show basic online indicator
	if (!networkScore) {
		return (
			<div
				className={`w-2 h-2 rounded-full bg-green-500 animate-pulse ${className}`}
			></div>
		);
	}

	// Online with network score - show signal bars
	const getSignalColor = () => {
		if (networkScore >= 4) return "text-green-500";
		if (networkScore === 3) return "text-yellow-500";
		return "text-orange-500";
	};

	const color = getSignalColor();

	return (
		<svg
			className={`w-4 h-4 ${color} ${className}`}
			viewBox="0 0 24 24"
			fill="currentColor"
		>
			{/* Signal bars based on score */}
			<rect
				x="2"
				y="18"
				width="3"
				height="4"
				opacity={networkScore >= 1 ? 1 : 0.3}
			/>
			<rect
				x="7"
				y="14"
				width="3"
				height="8"
				opacity={networkScore >= 2 ? 1 : 0.3}
			/>
			<rect
				x="12"
				y="10"
				width="3"
				height="12"
				opacity={networkScore >= 3 ? 1 : 0.3}
			/>
			<rect
				x="17"
				y="6"
				width="3"
				height="16"
				opacity={networkScore >= 4 ? 1 : 0.3}
			/>
			<rect
				x="22"
				y="2"
				width="3"
				height="20"
				opacity={networkScore >= 5 ? 1 : 0.3}
			/>
		</svg>
	);
};

export default NetworkQualityIndicator;
