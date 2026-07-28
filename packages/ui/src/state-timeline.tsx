import type { ReactNode } from "react";

/** A contiguous stretch of one state within a lane, in absolute time. */
export interface TimelineSpan {
	start: Date;
	/** Exclusive. Use the window end for a span that is still open. */
	end: Date;
	tone: TimelineTone;
	/** Shown in the browser tooltip on hover. */
	label?: string;
}

export interface TimelineLane {
	/** Stable identity for the lane; also the default row label. */
	name: string;
	label?: ReactNode;
	/** Spans that interrupt the lane's baseline. Order does not matter. */
	spans: TimelineSpan[];
	/** Trailing text on the right of the row, e.g. an uptime percentage. */
	trailing?: ReactNode;
}

export type TimelineTone = "ok" | "down" | "unknown";

const TONES: Record<TimelineTone, string> = {
	ok: "bg-green-500",
	down: "bg-red-500",
	unknown: "bg-gray-300",
};

const pct = (value: number) => `${(value * 100).toFixed(4)}%`;

/**
 * Horizontal state timeline: one lane per series, spans drawn as coloured bands
 * against a baseline.
 *
 * Positions are percentages of the window rather than pixels, so the chart is
 * fluid and needs no measurement pass or charting library. Spans are clipped to
 * `[from, to]`, which lets callers pass raw intervals that start before or end
 * after the window.
 */
export function StateTimeline({
	from,
	to,
	lanes,
	baseline = "ok",
	ticks = 5,
	formatTick = (d) =>
		d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
	labelWidth = "8rem",
	rowHeight = "1.25rem",
	emptyLabel = "No data",
}: {
	from: Date;
	to: Date;
	lanes: TimelineLane[];
	baseline?: TimelineTone;
	ticks?: number;
	formatTick?: (date: Date) => string;
	labelWidth?: string;
	rowHeight?: string;
	emptyLabel?: string;
}) {
	const startMs = from.getTime();
	const span = to.getTime() - startMs;

	if (!(span > 0) || lanes.length === 0) {
		return (
			<div className="py-8 text-center text-sm text-gray-500">{emptyLabel}</div>
		);
	}

	const tickDates = Array.from(
		{ length: Math.max(2, ticks) },
		(_, i) => new Date(startMs + (span * i) / (Math.max(2, ticks) - 1)),
	);

	return (
		<div className="space-y-1.5">
			{lanes.map((lane) => (
				<div key={lane.name} className="flex items-center gap-3">
					<div
						className="shrink-0 truncate font-mono text-xs text-gray-600"
						style={{ width: labelWidth }}
						title={lane.name}
					>
						{lane.label ?? lane.name}
					</div>

					<div
						className={`relative flex-1 overflow-hidden rounded-sm ${TONES[baseline]}`}
						style={{ height: rowHeight }}
					>
						{lane.spans.map((s) => {
							// Clip to the window so out-of-range intervals stay inside the track.
							const a = Math.max(s.start.getTime(), startMs);
							const b = Math.min(s.end.getTime(), startMs + span);
							if (b <= a) return null;
							const left = (a - startMs) / span;
							const width = (b - a) / span;
							return (
								<div
									key={`${s.start.getTime()}-${s.end.getTime()}`}
									className={`absolute inset-y-0 ${TONES[s.tone]}`}
									// A sub-pixel outage would round away to nothing, so keep a
									// visible floor: a short blip still needs to be clickable.
									style={{
										left: pct(left),
										width: `max(2px, ${pct(width)})`,
									}}
									title={s.label}
								/>
							);
						})}
					</div>

					{lane.trailing !== undefined && (
						<div className="w-16 shrink-0 text-right font-mono text-xs tabular-nums text-gray-600">
							{lane.trailing}
						</div>
					)}
				</div>
			))}

			<div
				className="flex justify-between pt-1 text-[11px] tabular-nums text-gray-400"
				style={{
					marginLeft: labelWidth,
					paddingLeft: "0.75rem",
					marginRight: lanes.some((l) => l.trailing !== undefined)
						? "4rem"
						: undefined,
				}}
			>
				{tickDates.map((d) => (
					<span key={d.getTime()}>{formatTick(d)}</span>
				))}
			</div>
		</div>
	);
}
