import { Badge, type BadgeVariant } from "@teton/smith-ui";
import type {
	DeploymentState,
	DeploymentSummary,
	DeviceCounts,
} from "@/app/api-client";

const STATES: Record<
	DeploymentState,
	{ label: string; variant: BadgeVariant }
> = {
	canary: { label: "Canary", variant: "purple" },
	rolling_out: { label: "Rolling out", variant: "blue" },
	paused: { label: "Paused", variant: "yellow" },
	completed: { label: "Deployed", variant: "green" },
	canceled: { label: "Canceled", variant: "gray" },
	failed: { label: "Failed", variant: "red" },
};

export const isActive = (state: DeploymentState) =>
	state === "canary" || state === "rolling_out" || state === "paused";

export function StateBadge({ state }: { state: DeploymentState }) {
	const { label, variant } = STATES[state];
	return (
		<Badge variant={variant} pill>
			{label}
		</Badge>
	);
}

/** Every device state, in the order a device moves through them. */
export const DEVICE_STATES = [
	{ key: "updated", label: "Updated", bar: "bg-green-500", variant: "green" },
	{ key: "updating", label: "Pending", bar: "bg-blue-500", variant: "blue" },
	{ key: "scheduled", label: "Waiting", bar: "bg-gray-300", variant: "gray" },
	{ key: "held", label: "Held", bar: "bg-yellow-400", variant: "yellow" },
	{
		key: "timed_out",
		label: "Timed out",
		bar: "bg-gray-300",
		variant: "gray",
	},
	{ key: "offline", label: "Offline", bar: "bg-orange-400", variant: "orange" },
	{
		key: "superseded",
		label: "Moved elsewhere",
		bar: "bg-gray-200",
		variant: "gray",
	},
	{ key: "canceled", label: "Canceled", bar: "bg-gray-200", variant: "gray" },
] as const;

export type DeviceStateKey = (typeof DEVICE_STATES)[number]["key"];

export function deviceState(key: string) {
	return DEVICE_STATES.find((state) => state.key === key) ?? DEVICE_STATES[2];
}

/** One bar for every device in a deployment or wave, split by what each is doing. */
export function ProgressBar({
	counts,
	className = "",
}: {
	counts: DeviceCounts;
	className?: string;
}) {
	return (
		<div
			className={`flex h-2 w-full overflow-hidden rounded-full bg-gray-100 ${className}`}
		>
			{DEVICE_STATES.map(({ key, bar, label }) => {
				const count = counts[key];
				if (!count || !counts.total) return null;
				return (
					<div
						key={key}
						className={bar}
						style={{ width: `${(count / counts.total) * 100}%` }}
						title={`${label}: ${count}`}
					/>
				);
			})}
		</div>
	);
}

/** Says what the bar's colours mean, with the numbers behind them. */
export function ProgressLegend({ counts }: { counts: DeviceCounts }) {
	return (
		<ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-gray-600">
			{DEVICE_STATES.filter(({ key }) => counts[key] > 0).map(
				({ key, label, bar }) => (
					<li key={key} className="flex items-center gap-1.5">
						<span className={`h-2 w-2 rounded-full ${bar}`} />
						<span className="tabular-nums">{counts[key]}</span> {label}
					</li>
				),
			)}
		</ul>
	);
}

/** The problems worth a glance on any progress row. */
/**
 * One verdict for a wave. Only failing services fail it: in a large fleet some
 * devices are always offline or slow, and they get there eventually.
 */
export function waveStatus(
	counts: DeviceCounts,
	opened: boolean,
): { label: string; variant: BadgeVariant; dot: string } {
	if (counts.unhealthy > 0) {
		return { label: "Failing", variant: "red", dot: "bg-red-500" };
	}
	if (!opened || counts.scheduled === counts.total) {
		return { label: "Waiting", variant: "gray", dot: "bg-gray-300" };
	}
	if (counts.updating > 0 || counts.scheduled > 0 || counts.held > 0) {
		return { label: "In progress", variant: "blue", dot: "bg-blue-500" };
	}
	return { label: "Done", variant: "green", dot: "bg-green-500" };
}

export function AttentionBadges({ counts }: { counts: DeviceCounts }) {
	return (
		<>
			{counts.unhealthy > 0 && (
				<Badge variant="red">{counts.unhealthy} unhealthy</Badge>
			)}
			{counts.offline > 0 && (
				<Badge variant="orange">{counts.offline} offline</Badge>
			)}
		</>
	);
}

export const formatClock = (date: string) =>
	new Date(date).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** What kind of deployment this is, and whether its release was withdrawn. */
export function DeploymentBadges({
	deployment,
}: {
	deployment: DeploymentSummary;
}) {
	return (
		<>
			{deployment.kind === "direct" && !deployment.release_candidate && (
				<Badge variant="gray">Selected devices</Badge>
			)}
			{deployment.release_candidate && (
				<Badge variant="orange">Release candidate</Badge>
			)}
			{deployment.release_yanked && <Badge variant="red">Yanked</Badge>}
			{deployment.is_latest && (
				<Badge variant="green" className="font-semibold">
					Latest
				</Badge>
			)}
		</>
	);
}

/** The one sentence that says where a deployment is and what happens next. */
export function statusLine(deployment: DeploymentSummary): string {
	const { state, canary, counts, next_wave_at, last_wave_at } = deployment;
	const left = counts.timed_out;
	switch (state) {
		case "canary":
			if (deployment.release_candidate) {
				return `${canary.updated} of ${canary.total} canary devices updated. Promote this candidate to a stable release before a fleet rollout.`;
			}
			return canary.updated < canary.total
				? `${canary.updated} of ${canary.total} canary devices updated. The fleet stays where it is until someone confirms.`
				: "Every canary device has updated. The fleet stays where it is until someone confirms.";
		case "rolling_out": {
			// Pending devices are timed out at this point; they still update on their own.
			const deadline = deployment.times_out_at
				? ` Devices not updated by ${formatClock(deployment.times_out_at)} time out.`
				: "";
			if (next_wave_at && last_wave_at) {
				return `${counts.updated} of ${counts.total} devices updated. Next wave opens at ${formatClock(next_wave_at)}, the last at ${formatClock(last_wave_at)}.${deadline}`;
			}
			return deployment.kind === "direct"
				? `${counts.updated} of ${counts.total} selected devices updated.${deadline}`
				: `${counts.updated} of ${counts.total} devices updated. Every wave is open.${deadline}`;
		}
		case "paused":
			return `${counts.updated} of ${counts.total} devices updated. Nothing else moves until it is resumed.`;
		case "completed":
			return left > 0
				? `${counts.updated} of ${counts.total} devices updated. ${counts.timed_out} timed out; they still update on their own once they come online.`
				: `${counts.updated} of ${counts.total} devices updated.`;
		case "canceled":
			return `Canceled with ${counts.updated} of ${counts.total} devices updated. Those stay on this release; the rest stayed where they were.`;
		case "failed":
			return "This deployment failed.";
	}
}
