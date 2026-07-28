import {
	Panel,
	SECTION_THEMES,
	StateTimeline,
	type TimelineLane,
} from "@teton/smith-ui";
import { Activity } from "lucide-react";
import { useMemo, useState } from "react";
import {
	UPTIME_RANGES,
	type UptimeRange,
	useDeviceUptime,
} from "./useDeviceUptime";

const formatDuration = (ms: number) => {
	const minutes = Math.round(ms / 60000);
	if (minutes < 60) return `${minutes}m`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h ${minutes % 60}m`;
	return `${Math.floor(hours / 24)}d ${hours % 24}h`;
};

const RangePicker = ({
	value,
	onChange,
}: {
	value: UptimeRange;
	onChange: (range: UptimeRange) => void;
}) => (
	<div className="flex items-center gap-0.5 rounded-md bg-white/60 p-0.5">
		{UPTIME_RANGES.map((r) => (
			<button
				key={r.label}
				type="button"
				onClick={() => onChange(r.label)}
				className={`rounded px-2 py-0.5 text-xs font-medium transition-colors ${
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

/**
 * Service availability timeline for the device overview: one lane per service,
 * red bands where an outage was recorded. Mirrors the Grafana uptime panel.
 */
const ServiceUptime = ({ serial }: { serial: string }) => {
	const [range, setRange] = useState<UptimeRange>("24h");
	const hours =
		UPTIME_RANGES.find((r) => r.label === range)?.hours ??
		UPTIME_RANGES[2].hours;
	const { data, isLoading } = useDeviceUptime(serial, hours);

	const { from, to, lanes } = useMemo(() => {
		if (!data) return { from: null, to: null, lanes: [] as TimelineLane[] };

		const from = new Date(data.from);
		const to = new Date(data.to);
		const windowMs = to.getTime() - from.getTime();

		const lanes = data.services.map((service): TimelineLane => {
			const spans = data.outages
				.filter((o) => o.service_name === service)
				.map((o) => {
					// An open outage runs to the end of the window; the API leaves
					// `ended_at` null rather than pinning it to a server clock.
					const start = new Date(o.started_at);
					const end = o.ended_at ? new Date(o.ended_at) : to;
					return {
						start,
						end,
						tone: "down" as const,
						label: `Down for ${formatDuration(
							Math.min(end.getTime(), to.getTime()) -
								Math.max(start.getTime(), from.getTime()),
						)}${o.ended_at ? "" : " (ongoing)"}`,
					};
				});

			// Clip before summing so an outage that predates the window doesn't
			// drag the percentage below what the chart actually shows.
			const downMs = spans.reduce(
				(acc, s) =>
					acc +
					Math.max(
						0,
						Math.min(s.end.getTime(), to.getTime()) -
							Math.max(s.start.getTime(), from.getTime()),
					),
				0,
			);
			const uptime = windowMs > 0 ? 1 - downMs / windowMs : 1;

			return {
				name: service,
				spans,
				trailing: `${(uptime * 100).toFixed(1)}%`,
			};
		});

		return { from, to, lanes };
	}, [data]);

	return (
		<Panel
			icon={Activity}
			title="Service Uptime"
			theme={SECTION_THEMES.green}
			actions={<RangePicker value={range} onChange={setRange} />}
		>
			{isLoading ? (
				<div className="space-y-2">
					{[0, 1, 2].map((i) => (
						<div key={i} className="h-5 animate-pulse rounded bg-gray-100" />
					))}
				</div>
			) : from && to ? (
				<StateTimeline
					from={from}
					to={to}
					lanes={lanes}
					formatTick={(d) =>
						hours > 48
							? d.toLocaleDateString([], { month: "short", day: "numeric" })
							: d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
					}
					emptyLabel="No services reported for this device"
				/>
			) : (
				<div className="py-8 text-center text-sm text-gray-500">
					Uptime unavailable
				</div>
			)}
		</Panel>
	);
};

export default ServiceUptime;
