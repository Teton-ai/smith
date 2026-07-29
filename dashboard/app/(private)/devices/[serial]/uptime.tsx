import { useQuery } from "@tanstack/react-query";
import {
	AlertBanner,
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

export const formatDuration = (ms: number) => {
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

type ReachabilitySummary = ReturnType<typeof summarize>;

/** Anything below this over a day is flapping worth acting on, not noise. */
const DEGRADED_RATIO = 0.99;
const ALERT_HOURS = 24;

/**
 * The last day's reachability, but only when it's bad enough to lead with —
 * null while loading and on a device that stayed put. Kept as a hook so the
 * overview can decide whether it has anything to report before rendering.
 */
export const useReachabilityProblem = (serial: string) => {
	const { data, isLoading } = useDeviceUptime(serial, ALERT_HOURS);

	const problem = useMemo(() => {
		if (!data) return null;
		const summary = summarize(data, SMITHD_SERVICE_NAME);
		return summary.ratio < DEGRADED_RATIO ? summary : null;
	}, [data]);

	// Callers need the pending state: "all clear" is a claim, and it shouldn't be
	// made before this check has answered.
	return { problem, isLoading };
};

/**
 * When each still-open service outage began, keyed by service name — so the
 * overview can say how long a dead unit has been dead. smithd is left out: that
 * lane is reachability, which has its own alert. Shares the query above with
 * `useReachabilityProblem`, so it costs no extra request; a service that broke
 * before the window still reports its true start, but one that has been down
 * longer than the API keeps outages simply won't appear.
 */
export const useOpenServiceOutages = (serial: string) => {
	const { data } = useDeviceUptime(serial, ALERT_HOURS);

	return useMemo(() => {
		const started = new Map<string, Date>();
		for (const outage of data?.outages ?? []) {
			if (outage.ended_at || outage.service_name === SMITHD_SERVICE_NAME)
				continue;
			const start = new Date(outage.started_at);
			const known = started.get(outage.service_name);
			if (!known || start < known) started.set(outage.service_name, start);
		}
		return started;
	}, [data]);
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

/** The worst single stretch of silence, clipped to the window. */
const longestGap = (summary: ReachabilitySummary) =>
	summary.spans.reduce<{ ms: number; start: Date } | null>((worst, span) => {
		const start = Math.max(span.start.getTime(), summary.from.getTime());
		const ms = Math.min(span.end.getTime(), summary.to.getTime()) - start;
		return worst && worst.ms >= ms ? worst : { ms, start: new Date(start) };
	}, null);

/**
 * Flags a device the API keeps losing. Same data as the Reachability panel
 * below, cut to the headline: how much of the day we heard from it, the worst
 * gap, and the bars so the shape of the drops is visible.
 */
export const ReachabilityAlert = ({
	summary,
}: {
	summary: ReachabilitySummary;
}) => {
	const worst = longestGap(summary);
	const since = summary.since;

	return (
		<AlertBanner
			tone={since ? "red" : "amber"}
			title={
				since
					? `Offline for ${formatDuration(Date.now() - since.getTime())}`
					: `Dropped offline ${summary.count}× in the last ${ALERT_HOURS}h`
			}
		>
			<div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
				<span className="tabular-nums">
					{(summary.ratio * 100).toFixed(1)}% reachable
				</span>
				{worst && (
					<span className="text-gray-500">
						· longest gap {formatDuration(worst.ms)} at{" "}
						{clock(worst.start, ALERT_HOURS)}
					</span>
				)}
			</div>

			<div className="mt-3">
				<UptimeBars
					from={summary.from}
					to={summary.to}
					spans={summary.spans}
					height="1.5rem"
					renderTooltip={bucketTooltip(ALERT_HOURS)}
				/>
				<div className="mt-1.5 flex justify-between text-[11px] tabular-nums text-gray-400">
					<span>{ALERT_HOURS}h ago</span>
					<span>now</span>
				</div>
			</div>
		</AlertBanner>
	);
};

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
