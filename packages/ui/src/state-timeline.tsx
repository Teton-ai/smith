import { type ReactNode, useState } from "react";

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

export interface UptimeBucket {
	start: Date;
	end: Date;
	/** Downtime inside this bucket, in ms. */
	downMs: number;
	/** Unobserved time inside this bucket (before `coverageFrom`), in ms. */
	unknownMs: number;
	/** Fraction of the *observed* part of the bucket spent up, 0..1. */
	ratio: number;
}

/**
 * Status-page style availability bars: the window is split into equal buckets,
 * each drawn green with a red portion sized to the downtime inside it.
 *
 * The proportion is the point — colouring a whole bucket red for a 30-second
 * blip makes a flapping device look permanently offline. A red sliver reads as
 * "briefly down", a full red bar as "down the whole time", with no intermediate
 * colour whose meaning has to be guessed.
 */
export function UptimeBars({
	from,
	to,
	spans,
	coverageFrom,
	buckets = 48,
	height = "1.75rem",
	renderTooltip,
}: {
	from: Date;
	to: Date;
	/** Down intervals in absolute time; clipped to the window. */
	spans: TimelineSpan[];
	/** Start of observed time. Anything before this, within the window, is drawn
	 * as unknown rather than up — omit when the whole window is observed. */
	coverageFrom?: Date;
	buckets?: number;
	height?: string;
	/** Hover card contents for a bucket. No tooltip is shown when omitted. */
	renderTooltip?: (bucket: UptimeBucket) => ReactNode;
}) {
	const [hovered, setHovered] = useState<number | null>(null);

	const startMs = from.getTime();
	const total = to.getTime() - startMs;

	if (!(total > 0)) return null;

	const coverageMs = coverageFrom?.getTime();
	const width = total / buckets;
	const items = Array.from({ length: buckets }, (_, i): UptimeBucket => {
		const a = startMs + i * width;
		const b = a + width;
		const unknownMs =
			coverageMs !== undefined ? Math.max(0, Math.min(b, coverageMs) - a) : 0;
		let downMs = 0;
		for (const s of spans) {
			const lo = Math.max(s.start.getTime(), a);
			const hi = Math.min(s.end.getTime(), b);
			if (hi > lo) downMs += hi - lo;
		}
		const observedWidth = Math.max(1, width - unknownMs);
		return {
			start: new Date(a),
			end: new Date(b),
			downMs,
			unknownMs,
			ratio: 1 - downMs / observedWidth,
		};
	});

	// Anchoring: centred on the bar, but pinned to whichever edge it would
	// otherwise overflow, since the card around it clips horizontally.
	const anchor = (index: number) => {
		const center = (index + 0.5) / buckets;
		if (center < 0.15) return { left: 0 };
		if (center > 0.85) return { right: 0 };
		return { left: `${center * 100}%`, transform: "translateX(-50%)" };
	};

	return (
		<div className="relative">
			{hovered !== null && renderTooltip && (
				<div
					className="pointer-events-none absolute bottom-full z-10 mb-2 whitespace-nowrap rounded-md bg-gray-900 px-2 py-1.5 text-[11px] leading-tight text-white shadow-lg"
					style={anchor(hovered)}
				>
					{renderTooltip(items[hovered])}
				</div>
			)}

			<div
				className="flex items-stretch gap-[2px]"
				style={{ height }}
				onMouseLeave={() => setHovered(null)}
			>
				{items.map((bucket, i) => (
					<div
						key={bucket.start.getTime()}
						onMouseEnter={() => setHovered(i)}
						className={`relative z-10 min-w-[2px] flex-1 cursor-pointer overflow-hidden rounded-[2px] bg-emerald-500 transition-opacity ${
							hovered === i ? "opacity-60" : ""
						}`}
					>
						{bucket.unknownMs > 0 && (
							<div
								className="absolute inset-y-0 left-0 bg-gray-300"
								style={{ width: `${(bucket.unknownMs / width) * 100}%` }}
							/>
						)}
						{bucket.downMs > 0 && (
							<div
								className="absolute inset-x-0 bottom-0 bg-red-500"
								// Floored so a blip too small to round to a pixel is still seen.
								style={{
									height: `max(3px, ${(bucket.downMs / width) * 100}%)`,
								}}
							/>
						)}
					</div>
				))}
			</div>
		</div>
	);
}

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
