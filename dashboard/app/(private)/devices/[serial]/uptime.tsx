import { useQuery } from "@tanstack/react-query";
import {
	Panel,
	SECTION_THEMES,
	type TimelineSpan,
	UptimeBars,
	type UptimeBucket,
} from "@teton/smith-ui";
import { Wifi, WifiOff } from "lucide-react";
import { useMemo, useState } from "react";
import { useClientMutator } from "@/app/api-client-mutator";

interface ServiceOutage {
	service_name: string;
	started_at: string;
	/** Null while the outage is still open. */
	ended_at: string | null;
}

interface DeviceUptime {
	from: string;
	to: string;
	services: string[];
	outages: ServiceOutage[];
}

/**
 * Reachability is stored as an outage of smithd itself (see
 * `api/src/device/mod.rs`), so this lane means "the device could talk to the
 * API", not "the systemd unit was running" — a device that is gone cannot
 * report its own silence, the API infers it. Per-service health is a different
 * thing entirely and is not shown here.
 */
const SMITHD_SERVICE_NAME = "smithd";

const UPTIME_RANGES = [
	{ label: "1h", hours: 1 },
	{ label: "6h", hours: 6 },
	{ label: "24h", hours: 24 },
	{ label: "7d", hours: 24 * 7 },
] as const;

type UptimeRange = (typeof UPTIME_RANGES)[number]["label"];

const useDeviceUptime = (deviceId: string, hours: number) => {
	const fetcher = useClientMutator<DeviceUptime>();

	return useQuery({
		queryKey: ["deviceUptime", deviceId, hours],
		queryFn: () => {
			// Anchored per fetch rather than per render so the window doesn't shift
			// on every re-render and defeat the query cache.
			const to = new Date();
			const from = new Date(to.getTime() - hours * 3600_000);
			return fetcher({
				url: `/devices/${deviceId}/uptime`,
				method: "GET",
				params: { from: from.toISOString(), to: to.toISOString() },
			});
		},
		enabled: !!deviceId,
		refetchInterval: 60000,
	});
};

const formatDuration = (ms: number) => {
	const minutes = Math.round(ms / 60000);
	if (minutes < 1) return "<1m";
	if (minutes < 60) return `${minutes}m`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h ${minutes % 60}m`;
	return `${Math.floor(hours / 24)}d ${hours % 24}h`;
};

/**
 * Turns the raw outage rows into drawable spans plus the headline numbers.
 * Everything is clipped to `[from, to]` first, so an outage that predates the
 * window can't drag the percentage below what the bars actually show.
 */
const summarize = (data: DeviceUptime, service: string) => {
	const from = new Date(data.from);
	const to = new Date(data.to);
	const windowMs = to.getTime() - from.getTime();

	let downMs = 0;
	let since: Date | null = null;

	const spans = data.outages
		.filter((o) => o.service_name === service)
		.map((o): TimelineSpan => {
			// An open outage runs to the end of the window; the API leaves
			// `ended_at` null rather than pinning it to a server clock.
			const start = new Date(o.started_at);
			const end = o.ended_at ? new Date(o.ended_at) : to;
			downMs += Math.max(
				0,
				Math.min(end.getTime(), to.getTime()) -
					Math.max(start.getTime(), from.getTime()),
			);
			if (!o.ended_at) since = start;
			return { start, end, tone: "down" };
		});

	return {
		from,
		to,
		spans,
		downMs,
		ratio: windowMs > 0 ? 1 - downMs / windowMs : 1,
		count: spans.length,
		since: since as Date | null,
	};
};

const RangePicker = ({
	value,
	onChange,
}: {
	value: UptimeRange;
	onChange: (range: UptimeRange) => void;
}) => (
	<div className="flex items-center gap-0.5 rounded-md bg-black/5 p-0.5">
		{UPTIME_RANGES.map((r) => (
			<button
				key={r.label}
				type="button"
				onClick={() => onChange(r.label)}
				className={`rounded px-2 py-0.5 text-xs font-medium transition-colors cursor-pointer ${
					r.label === value
						? "bg-white text-gray-900 shadow-sm"
						: "text-gray-500 hover:text-gray-900"
				}`}
			>
				{r.label}
			</button>
		))}
	</div>
);

/** Labels get coarser than clock time once the window spans days. */
const clock = (d: Date, hours: number) =>
	hours > 48
		? d.toLocaleString([], {
				month: "short",
				day: "numeric",
				hour: "2-digit",
				minute: "2-digit",
			})
		: d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** Two-line hover card: which slice of time, and what happened in it. */
const bucketTooltip = (hours: number) => (bucket: UptimeBucket) => (
	<>
		<div className="font-medium tabular-nums">
			{clock(bucket.start, hours)} – {clock(bucket.end, hours)}
		</div>
		<div className={bucket.downMs > 0 ? "text-red-300" : "text-emerald-300"}>
			{bucket.downMs > 0
				? `Unreachable ${formatDuration(bucket.downMs)} of this slice`
				: "Reachable throughout"}
		</div>
	</>
);

/**
 * Reachability panel for the device overview: how much of the window the device
 * checked in with the API, as a headline percentage plus one bar per time slice.
 *
 * Distinct from the Network Connections panel below it — that reports interface
 * state as the device sees it, this reports whether we heard from it at all.
 */
export const DeviceReachability = ({ serial }: { serial: string }) => {
	const [range, setRange] = useState<UptimeRange>("24h");
	const hours = UPTIME_RANGES.find((r) => r.label === range)?.hours ?? 24;
	const { data, isLoading } = useDeviceUptime(serial, hours);

	const summary = useMemo(
		() => (data ? summarize(data, SMITHD_SERVICE_NAME) : null),
		[data],
	);

	// Only used to colour the panel: an open outage means the device is silent
	// right now, which the whole card should signal at a glance.
	const down = Boolean(summary?.since);
	const stats =
		summary && summary.count > 0
			? `${summary.count} interruption${summary.count === 1 ? "" : "s"} · ${formatDuration(summary.downMs)} total`
			: "no interruptions";

	return (
		<Panel
			icon={down ? WifiOff : Wifi}
			theme={down ? SECTION_THEMES.red : SECTION_THEMES.green}
			title="Reachability"
			actions={<RangePicker value={range} onChange={setRange} />}
			bodyClassName="p-4"
		>
			<p className="mb-3 text-xs text-gray-400">
				How much of the window the device checked in with the API
			</p>

			{isLoading || !summary ? (
				<div className="h-12 animate-pulse rounded bg-gray-100" />
			) : (
				<div className="flex items-end gap-4">
					<div className="shrink-0">
						<div className="font-mono text-2xl font-semibold leading-none tabular-nums text-gray-900">
							{(summary.ratio * 100).toFixed(2)}
							<span className="text-base text-gray-400">%</span>
						</div>
						<div className="mt-1.5 text-[11px] uppercase tracking-wide text-gray-400">
							reachable · last {range}
						</div>
					</div>

					<div className="min-w-0 flex-1">
						<UptimeBars
							from={summary.from}
							to={summary.to}
							spans={summary.spans}
							renderTooltip={bucketTooltip(hours)}
						/>
						<div className="mt-1.5 flex items-center justify-between gap-3 text-[11px] tabular-nums text-gray-400">
							<span>{clock(summary.from, hours)}</span>
							<span className="truncate text-gray-500">{stats}</span>
							{/* No live-status label here: the device header already carries
							    the online dot and last-seen. This panel is history. */}
							<span>now</span>
						</div>
					</div>
				</div>
			)}
		</Panel>
	);
};
