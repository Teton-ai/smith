import { Layers } from "lucide-react";
import moment from "moment";
import { useEffect, useState } from "react";
import {
	type DeploymentWave,
	useApiGetDeploymentWaves,
} from "@/app/api-client";
import { Tooltip } from "@/app/components/tooltip";

const formatClock = (date: string) =>
	new Date(date).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

type WavePhase = "scheduled" | "in-progress" | "complete";

const wavePhase = (wave: DeploymentWave, now: number): WavePhase => {
	if (new Date(wave.opens_at).getTime() > now) return "scheduled";
	// Superseded devices left this deployment, so they no longer hold a wave open.
	return wave.updated + wave.superseded >= wave.total
		? "complete"
		: "in-progress";
};

const PHASE_BADGE: Record<WavePhase, string> = {
	scheduled: "text-gray-700 bg-gray-100 border-gray-200",
	"in-progress": "text-blue-700 bg-blue-100 border-blue-200",
	complete: "text-green-700 bg-green-100 border-green-200",
};

/** Bar segments, in display order. `unhealthy` is carved out of `updated`. */
const SEGMENTS = [
	{
		key: "healthy",
		label: "Updated",
		bar: "bg-green-500",
		dot: "bg-green-500",
	},
	{
		key: "unhealthy",
		label: "Unhealthy",
		bar: "bg-red-500",
		dot: "bg-red-500",
	},
	{
		key: "updating",
		label: "Updating",
		bar: "bg-blue-500",
		dot: "bg-blue-500",
	},
	{
		key: "offline",
		label: "Offline",
		bar: "bg-amber-400",
		dot: "bg-amber-400",
	},
	{
		key: "scheduled",
		label: "Waiting for wave",
		bar: "bg-gray-200",
		dot: "bg-gray-300",
	},
	{
		key: "superseded",
		label: "Moved to another release",
		bar: "bg-gray-400",
		dot: "bg-gray-400",
	},
] as const;

type SegmentKey = (typeof SEGMENTS)[number]["key"];

const segmentCounts = (wave: DeploymentWave): Record<SegmentKey, number> => {
	const unhealthy = Math.min(wave.unhealthy, wave.updated);
	return {
		healthy: wave.updated - unhealthy,
		unhealthy,
		updating: wave.updating,
		offline: wave.offline,
		scheduled: wave.scheduled,
		superseded: wave.superseded,
	};
};

const WaveRow = ({
	wave,
	now,
	canaryLabel,
	selected,
	onSelect,
}: {
	wave: DeploymentWave;
	now: number;
	canaryLabel: string;
	selected: boolean;
	onSelect: () => void;
}) => {
	const phase = wavePhase(wave, now);
	const counts = segmentCounts(wave);
	const opensAt = moment(wave.opens_at);

	return (
		<tr
			className={`cursor-pointer text-sm ${selected ? "bg-blue-50" : "hover:bg-gray-50"}`}
			onClick={onSelect}
		>
			<th
				scope="row"
				className="px-4 py-2 text-left font-medium text-gray-900 whitespace-nowrap"
			>
				<button
					type="button"
					aria-pressed={selected}
					aria-controls="deployment-device-list"
					className="cursor-pointer rounded text-left text-blue-700 hover:underline"
					onClick={(event) => {
						event.stopPropagation();
						onSelect();
					}}
				>
					{wave.wave === 0 ? canaryLabel : `Wave ${wave.wave}`}
				</button>
			</th>
			<td className="px-4 py-2 text-xs text-gray-600 whitespace-nowrap">
				<Tooltip content={opensAt.format("MMM D, YYYY h:mm:ss A")}>
					<span>
						{phase === "scheduled"
							? `Opens ${formatClock(wave.opens_at)} (${opensAt.fromNow()})`
							: `Opened ${formatClock(wave.opens_at)}`}
					</span>
				</Tooltip>
			</td>
			<td className="px-4 py-2 w-full min-w-32">
				<div className="flex h-3 w-full overflow-hidden rounded-full bg-gray-100">
					{SEGMENTS.map(({ key, label, bar }) =>
						counts[key] > 0 ? (
							<div
								key={key}
								className={`${bar} h-full border-r border-white last:border-r-0`}
								style={{ width: `${(counts[key] / wave.total) * 100}%` }}
								title={`${label}: ${counts[key]}`}
							/>
						) : null,
					)}
				</div>
			</td>
			<td className="px-4 py-2 whitespace-nowrap">
				<span className="flex items-center gap-1">
					{counts.unhealthy > 0 && (
						<span className="px-1.5 py-0.5 rounded text-xs font-medium bg-red-100 text-red-700">
							{counts.unhealthy} unhealthy
						</span>
					)}
					{wave.offline > 0 && (
						<span className="px-1.5 py-0.5 rounded text-xs font-medium bg-amber-100 text-amber-800">
							{wave.offline} offline
						</span>
					)}
					{wave.superseded > 0 && (
						<span className="px-1.5 py-0.5 rounded text-xs font-medium bg-gray-100 text-gray-700">
							{wave.superseded} moved
						</span>
					)}
				</span>
			</td>
			<td className="px-4 py-2 text-xs text-right text-gray-500 tabular-nums whitespace-nowrap">
				{wave.updated}/{wave.total - wave.superseded}
			</td>
			<td className="px-4 py-2 whitespace-nowrap">
				<span
					className={`px-2 py-0.5 rounded-full text-xs font-medium border whitespace-nowrap ${PHASE_BADGE[phase]}`}
				>
					{phase === "scheduled"
						? "Scheduled"
						: phase === "complete"
							? "Complete"
							: "In progress"}
				</span>
			</td>
		</tr>
	);
};

/** Wave-by-wave progress of a release's deployment. Wave 0 is the canary. */
export const DeploymentWaves = ({
	releaseId,
	releaseCandidate,
	selectedWave,
	onSelectWave,
}: {
	releaseId: number;
	releaseCandidate: boolean;
	selectedWave: number | null;
	onSelectWave: (wave: number) => void;
}) => {
	const { data: waves = [] } = useApiGetDeploymentWaves(releaseId, {
		query: { refetchInterval: 5000 },
	});

	// Re-render so "opens in …" and scheduled → open transitions stay current
	// between refetches.
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		const timer = setInterval(() => setNow(Date.now()), 15_000);
		return () => clearInterval(timer);
	}, []);

	if (waves.length === 0) return null;

	const total = waves.reduce((n, w) => n + w.total - w.superseded, 0);
	const updated = waves.reduce((n, w) => n + w.updated, 0);
	const openWaves = waves.filter((w) => wavePhase(w, now) !== "scheduled");
	const nextWave = waves.find((w) => wavePhase(w, now) === "scheduled");

	return (
		<div className="overflow-hidden rounded-xl border border-gray-200 bg-white">
			<div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
				<div className="flex items-center space-x-2">
					<Layers className="w-5 h-5 text-gray-700" />
					<h3 className="font-semibold text-gray-900">Rollout waves</h3>
				</div>
				<div className="text-right text-sm text-gray-600">
					<div>
						<span className="font-semibold text-gray-900 tabular-nums">
							{updated}/{total}
						</span>{" "}
						devices updated
					</div>
					<div className="text-xs text-gray-500">
						{openWaves.length} of {waves.length} waves open
						{nextWave &&
							` · next opens ${formatClock(nextWave.opens_at)} (${moment(nextWave.opens_at).fromNow()})`}
					</div>
				</div>
			</div>

			<div className="overflow-x-auto">
				<table className="min-w-full text-left" aria-label="Rollout progress">
					<thead>
						<tr className="border-y border-gray-100 text-xs text-gray-500">
							<th className="px-4 py-2 font-medium">Group</th>
							<th className="px-4 py-2 font-medium">Schedule</th>
							<th className="px-4 py-2 font-medium">Progress</th>
							<th className="px-4 py-2 font-medium">Attention</th>
							<th className="px-4 py-2 font-medium text-right">Updated</th>
							<th className="px-4 py-2 font-medium">Status</th>
						</tr>
					</thead>
					<tbody className="divide-y divide-gray-100">
						{waves.map((wave) => (
							<WaveRow
								key={wave.wave}
								wave={wave}
								now={now}
								canaryLabel={releaseCandidate ? "Selected" : "Canary"}
								selected={selectedWave === wave.wave}
								onSelect={() => onSelectWave(wave.wave)}
							/>
						))}
					</tbody>
				</table>
			</div>

			<div className="flex flex-wrap gap-x-4 gap-y-1 border-t border-gray-100 px-4 py-3 text-xs text-gray-500">
				{SEGMENTS.map(({ key, label, dot }) => (
					<span key={key} className="inline-flex items-center gap-1.5">
						<span className={`inline-block w-2.5 h-2.5 rounded-full ${dot}`} />
						{label}
					</span>
				))}
			</div>
		</div>
	);
};
