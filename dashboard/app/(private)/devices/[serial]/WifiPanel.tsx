"use client";

import { useMutation } from "@tanstack/react-query";
import { Badge, Button, Panel, SECTION_THEMES } from "@teton/smith-ui";
import { Eye, EyeOff, RefreshCw, Wifi, WifiOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
	type ConfiguredNetwork,
	useGetConfiguredNetworksForDevice,
} from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";

const MASK = "••••••••••••";

interface WifiPanelProps {
	serial: string;
}

const WifiPanel = ({ serial }: WifiPanelProps) => {
	const [revealedIds, setRevealedIds] = useState<Set<string>>(new Set());
	const [syncing, setSyncing] = useState(false);
	const dispatchedAt = useRef<Date | null>(null);

	const {
		data: profiles,
		isLoading,
		isError,
	} = useGetConfiguredNetworksForDevice(serial, {
		query: { refetchInterval: syncing ? 3000 : false },
	});

	const currentNetwork = profiles?.find((p) => p.is_active);

	const fetcher = useClientMutator<void>();
	const { mutate: dispatchRefresh, isPending: isDispatching } = useMutation({
		mutationFn: () =>
			fetcher({
				url: `/devices/${serial}/commands`,
				method: "POST",
				data: [
					{ id: -6, command: "ReportNMProfiles", continue_on_error: false },
				],
			}),
		onSuccess: () => {
			setSyncing(true);
			dispatchedAt.current = new Date();
		},
	});

	// Fast path: clear syncing when the DB rows confirm the device responded.
	useEffect(() => {
		if (
			syncing &&
			dispatchedAt.current !== null &&
			profiles?.some((p) => new Date(p.updated_at) > dispatchedAt.current!)
		) {
			setSyncing(false);
			dispatchedAt.current = null;
		}
	}, [profiles, syncing]);

	// Timeout fallback: covers devices with no configured profiles (empty array
	// has no updated_at to compare) and offline devices.
	useEffect(() => {
		if (!syncing) return;
		const timer = setTimeout(() => {
			setSyncing(false);
			dispatchedAt.current = null;
		}, 45_000);
		return () => clearTimeout(timer);
	}, [syncing]);

	// Auto-trigger once after first successful data load, only if data is older than 12h.
	// Persisted per device in localStorage so the throttle survives remounts.
	const hasAutoDispatched = useRef(false);
	useEffect(() => {
		if (isLoading || isError || hasAutoDispatched.current) return;
		hasAutoDispatched.current = true;
		const staleMs = 12 * 60 * 60 * 1000; // 12 hours
		const newestAt =
			profiles && profiles.length > 0
				? Math.max(...profiles.map((p) => new Date(p.updated_at).getTime()))
				: 0;
		const storageKey = `wifi_auto_dispatch_${serial}`;
		const lastDispatch = Number(localStorage.getItem(storageKey) ?? 0);
		if (Date.now() - Math.max(newestAt, lastDispatch) > staleMs) {
			localStorage.setItem(storageKey, String(Date.now()));
			dispatchRefresh();
		}
	}, [isLoading, isError, profiles, dispatchRefresh, serial]);

	const toggleReveal = (profileName: string) => {
		setRevealedIds((prev) => {
			const next = new Set(prev);
			if (next.has(profileName)) {
				next.delete(profileName);
			} else {
				next.add(profileName);
			}
			return next;
		});
	};

	const isBusy = isLoading || isDispatching || syncing;

	return (
		<Panel
			title="WiFi"
			icon={Wifi}
			theme={SECTION_THEMES.orange}
			actions={
				<Button
					variant="soft"
					tone="gray"
					size="sm"
					loading={isDispatching || syncing}
					onClick={() => dispatchRefresh()}
					icon={<RefreshCw className="w-4 h-4" />}
				>
					{syncing ? "Syncing..." : "Refresh"}
				</Button>
			}
		>
			{/* Last checked */}
			{isError ? (
				<p className="text-sm text-red-500 mb-3">
					Failed to load WiFi profiles.
				</p>
			) : (
				<p className="text-sm text-gray-500 mb-3">
					{profiles && profiles.length > 0
						? `Last checked ${new Date(Math.max(...profiles.map((p) => new Date(p.updated_at).getTime()))).toLocaleString()}`
						: "Never checked"}
				</p>
			)}

			{/* Current network */}
			<div className="mb-4 pb-4 border-b border-gray-100">
				<p className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
					Current network
				</p>
				{isBusy && !currentNetwork ? (
					<div className="h-4 w-32 bg-gray-100 rounded animate-pulse" />
				) : currentNetwork ? (
					<div className="flex items-center gap-2">
						<Wifi className="w-4 h-4 text-green-500 flex-shrink-0" />
						<span className="text-sm font-medium text-gray-900">
							{currentNetwork.ssid}
						</span>
						<Badge variant="green" pill>
							Connected
						</Badge>
					</div>
				) : (
					<div className="flex items-center gap-2 text-gray-500">
						<WifiOff className="w-4 h-4 flex-shrink-0" />
						<span className="text-sm">Disconnected</span>
					</div>
				)}
			</div>

			{/* Configured profiles */}
			<div>
				<p className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
					Configured profiles
				</p>
				{isBusy && (!profiles || profiles.length === 0) ? (
					<div className="space-y-2">
						{[1, 2, 3].map((i) => (
							<div key={i} className="h-4 bg-gray-100 rounded animate-pulse" />
						))}
					</div>
				) : !profiles || profiles.length === 0 ? (
					<p className="text-sm text-gray-500">No profiles reported yet.</p>
				) : (
					<div className="divide-y divide-gray-100">
						{profiles.map((profile: ConfiguredNetwork) => {
							const revealed = revealedIds.has(profile.profile_name);
							return (
								<div
									key={profile.profile_name}
									className="py-3 flex flex-col gap-1"
								>
									<div className="flex items-center justify-between gap-2">
										<div className="flex flex-col min-w-0">
											<span className="text-sm font-medium text-gray-900 truncate">
												{profile.profile_name}
											</span>
											{profile.ssid && (
												<span className="text-xs text-gray-500 font-mono truncate">
													{profile.ssid}
												</span>
											)}
										</div>
										<div className="flex items-center gap-2 flex-shrink-0">
											{profile.is_active && (
												<Badge variant="green" pill>
													Active
												</Badge>
											)}
										</div>
									</div>
									{profile.password && (
										<div className="flex items-center gap-2">
											<span className="font-mono text-sm text-gray-700">
												{revealed ? profile.password : MASK}
											</span>
											<button
												type="button"
												onClick={() => toggleReveal(profile.profile_name)}
												className="text-gray-400 hover:text-gray-600"
												aria-label={`${revealed ? "Hide" : "Reveal"} password for ${profile.profile_name}`}
												aria-pressed={revealed}
											>
												{revealed ? (
													<EyeOff className="w-3.5 h-3.5" />
												) : (
													<Eye className="w-3.5 h-3.5" />
												)}
											</button>
										</div>
									)}
								</div>
							);
						})}
					</div>
				)}
			</div>
		</Panel>
	);
};

export default WifiPanel;
