import { keepPreviousData, useQueryClient } from "@tanstack/react-query";
import {
	AlertBanner,
	BackLink,
	Badge,
	Button,
	Card,
	LabelChip,
	PageContainer,
	Panel,
	SECTION_THEMES,
} from "@teton/smith-ui";
import {
	Check,
	CheckCircle2,
	Circle,
	History,
	Layers,
	Rocket,
	Search,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import {
	type DeploymentDetail,
	type DeploymentEventInfo,
	type DeviceCounts,
	getApiListDeploymentDevicesQueryKey,
	useApiGetDeployment,
	useApiListDeploymentDevices,
	useApiPromoteDeployment,
	usePromoteRelease,
	type WaveProgress,
} from "@/app/api-client";
import { ArchitectureIcon } from "@/app/components/ArchitectureIcon";
import { Modal } from "@/app/components/modal";
import { RelativeTime } from "@/app/components/RelativeTime";
import { requestErrorMessage } from "@/app/utils/release";
import {
	AttentionBadges,
	DEVICE_STATES,
	DeploymentBadges,
	type DeviceStateKey,
	deviceState,
	formatClock,
	ProgressBar,
	ProgressLegend,
	StateBadge,
	statusLine,
	waveStatus,
} from "../shared";

const STEP_DOT = {
	done: "bg-green-500 text-white",
	current: "bg-blue-600 text-white",
	paused: "bg-yellow-400 text-white",
	stopped: "bg-gray-400 text-white",
	upcoming: "bg-gray-100 text-gray-400",
};

/** Where the deployment sits in its journey, at a glance. */
const Steps = ({ detail }: { detail: DeploymentDetail }) => {
	const { state, kind, release_candidate, promoted_at } = detail.summary;
	const reachedRollout = kind !== "staged" || promoted_at != null;
	const ended = state === "canceled" || state === "failed";
	const step = (reached: boolean, doing: boolean, done: boolean) => {
		if (done) return STEP_DOT.done;
		if (!reached) return STEP_DOT.upcoming;
		if (ended) return STEP_DOT.stopped;
		if (state === "paused" && doing) return STEP_DOT.paused;
		return doing ? STEP_DOT.current : STEP_DOT.upcoming;
	};
	const completed = state === "completed";
	const audience = "Fleet";
	// A direct deployment is one step: its devices update at once.
	const steps =
		kind !== "staged"
			? [
					{
						label: kind === "rollback" ? "Rollback" : "Selected devices",
						className: step(true, !completed, completed),
						done: completed,
					},
				]
			: [
					{
						label: "Canary",
						className: step(true, !reachedRollout, reachedRollout),
						done: reachedRollout,
					},
					{
						label: "Staggered rollout",
						className: step(reachedRollout, !completed, completed),
						done: completed,
					},
					{
						label: audience,
						className: step(completed, false, completed),
						done: completed,
					},
				];

	return (
		<ol className="flex flex-wrap items-center gap-x-3 gap-y-2">
			{steps.map((item, index) => (
				<li key={item.label} className="flex items-center gap-2 text-sm">
					{index > 0 && <span className="w-6 h-px bg-gray-300" />}
					<span
						className={`flex h-5 w-5 items-center justify-center rounded-full ${item.className}`}
					>
						{item.done ? (
							<Check className="w-3 h-3" />
						) : (
							<Circle className="w-2 h-2 fill-current" />
						)}
					</span>
					<span className="font-medium text-gray-700">{item.label}</span>
				</li>
			))}
		</ol>
	);
};

/** Preserve the existing RC-to-stable release promotion before pilot rollouts. */
const PromoteCandidate = ({ detail }: { detail: DeploymentDetail }) => {
	const navigate = useNavigate();
	const [version, setVersion] = useState(
		detail.summary.release_version.replace(/-rc.*$/, ""),
	);
	const [error, setError] = useState<string | null>(null);
	const promotion = usePromoteRelease({
		mutation: {
			onSuccess: (id) => navigate(`/releases/${id}`),
			onError: (error) => setError(requestErrorMessage(error)),
		},
	});
	return (
		<div className="space-y-3">
			<p className="text-sm text-gray-700">
				Release candidates cannot roll out to the fleet. Promote this candidate
				to a stable release to continue.
			</p>
			<label className="block text-sm text-gray-700">
				Stable release version
				<input
					value={version}
					onChange={(event) => setVersion(event.target.value)}
					className="ml-2 rounded border border-gray-300 px-2 py-1"
				/>
			</label>
			<Button
				tone="blue"
				loading={promotion.isPending}
				disabled={!version.trim()}
				onClick={() =>
					promotion.mutate({
						releaseId: detail.summary.release_id,
						data: { version: version.trim() },
					})
				}
			>
				Promote to release
			</Button>
			{error && (
				<p role="alert" className="text-sm text-red-700">
					{error}
				</p>
			)}
		</div>
	);
};

/** What stands between the canary and the fleet, as the API sees it. */
const CanaryChecklist = ({
	detail,
	onPromote,
}: {
	detail: DeploymentDetail;
	onPromote: () => void;
}) => {
	const { actions, summary } = detail;
	if (summary.release_candidate) return <PromoteCandidate detail={detail} />;
	return (
		<div className="space-y-3">
			{actions.promote_blockers.length === 0 ? (
				<p className="flex items-center gap-2 text-sm text-gray-900">
					<CheckCircle2 className="w-4 h-4 text-green-500 shrink-0" />
					Every canary device has updated and its services are healthy.
				</p>
			) : (
				<ul className="space-y-1.5 text-sm text-gray-600">
					{actions.promote_blockers.map((blocker) => (
						<li key={blocker} className="flex items-start gap-2">
							<Circle className="w-4 h-4 text-gray-300 shrink-0 mt-0.5" />
							{blocker}
						</li>
					))}
				</ul>
			)}
			<Button
				variant="solid"
				tone="blue"
				icon={<Rocket className="w-4 h-4" />}
				disabled={!actions.can_promote}
				title={
					actions.can_promote
						? undefined
						: (actions.denied_reason ?? "The canary isn't ready yet")
				}
				onClick={onPromote}
			>
				{summary.release_candidate
					? "Confirm rollout to pilot group"
					: "Confirm rollout to fleet"}
			</Button>
		</div>
	);
};

const WaveRow = ({
	wave,
	paused,
	selected,
	onSelect,
}: {
	wave: WaveProgress;
	paused: boolean;
	selected: boolean;
	onSelect: () => void;
}) => {
	const opened = new Date(wave.opens_at).getTime() <= Date.now();
	const status = waveStatus(wave.counts, opened);
	const when = paused
		? opened
			? "Opened"
			: "Waiting"
		: opened
			? `Opened ${formatClock(wave.opens_at)}`
			: `Opens ${formatClock(wave.opens_at)}`;
	return (
		<tr
			onClick={onSelect}
			className={`cursor-pointer text-sm transition-colors ${selected ? "bg-blue-50" : "hover:bg-gray-50"}`}
		>
			<th
				scope="row"
				className="px-4 py-2 text-left font-medium text-gray-900 whitespace-nowrap"
			>
				<button
					type="button"
					onClick={(event) => {
						event.stopPropagation();
						onSelect();
					}}
					aria-pressed={selected}
					aria-controls="rollout-devices"
					className="cursor-pointer rounded text-left hover:text-blue-600 focus-visible:outline-2 focus-visible:outline-blue-600"
					title={selected ? "Show all devices" : "Show devices in this group"}
				>
					<span className="inline-flex items-center gap-2">
						<span
							className={`h-2 w-2 rounded-full shrink-0 ${status.dot}`}
							title={status.label}
						/>
						{wave.wave === 0 ? "Initial group" : `Wave ${wave.wave}`}
					</span>
				</button>
			</th>
			<td
				className={`px-4 py-2 text-xs whitespace-nowrap ${opened ? "text-gray-600" : "text-gray-400"}`}
			>
				{when}
			</td>
			<td className="px-4 py-2 w-full min-w-32">
				<ProgressBar counts={wave.counts} />
			</td>
			<td className="px-4 py-2 whitespace-nowrap">
				<span className="flex items-center gap-1">
					<Badge variant={status.variant}>{status.label}</Badge>
					<AttentionBadges counts={wave.counts} />
				</span>
			</td>
			<td className="px-4 py-2 text-xs text-right text-gray-500 tabular-nums">
				{wave.counts.updated}/{wave.counts.total}
			</td>
		</tr>
	);
};

const EVENT_LABELS: Record<string, string> = {
	started: "Started",
	promoted: "Confirmed rollout",
	paused: "Paused",
	resumed: "Resumed",
	canceled: "Canceled",
	rushed: "Rushed rollout",
	finalized: "Finalized",
	timed_out: "Stopped waiting",
	yanked: "Release yanked",
};

const Activity = ({ events }: { events: DeploymentEventInfo[] }) => {
	if (events.length === 0) {
		return (
			<p className="text-sm text-gray-500">
				No activity recorded for this deployment.
			</p>
		);
	}
	return (
		<ol className="space-y-3">
			{events.map((event) => (
				<li key={event.id} className="text-sm">
					<div className="font-medium text-gray-900">
						{EVENT_LABELS[event.kind] ?? event.kind}
					</div>
					{event.detail && <div className="text-gray-600">{event.detail}</div>}
					<div className="text-xs text-gray-400">
						{event.user_email ?? "System"} ·{" "}
						<RelativeTime date={event.created_at} />
					</div>
				</li>
			))}
		</ol>
	);
};

/** How many devices the table shows; the totals come from `counts`. */
const DEVICE_SAMPLE = 25;

/**
 * A sample of the deployment's devices: filtering happens in the API, so
 * filters find devices beyond the first page of thousands, and `counts` (the
 * deployment's or the wave's exact totals) label the filters.
 */
const DeviceTable = ({
	deploymentId,
	counts,
	wave,
	onClearWave,
}: {
	deploymentId: number;
	counts: DeviceCounts;
	wave: number | null;
	onClearWave: () => void;
}) => {
	const [state, setState] = useState<DeviceStateKey | "unhealthy" | null>(null);
	const [search, setSearch] = useState("");
	const [query, setQuery] = useState("");
	useEffect(() => {
		const timer = setTimeout(() => setQuery(search.trim()), 300);
		return () => clearTimeout(timer);
	}, [search]);

	const { data: visible = [] } = useApiListDeploymentDevices(
		deploymentId,
		{
			limit: DEVICE_SAMPLE,
			...(state ? { state } : {}),
			...(wave !== null ? { wave } : {}),
			...(query ? { search: query } : {}),
		},
		{ query: { refetchInterval: 5000, placeholderData: keepPreviousData } },
	);
	const filters = [
		...DEVICE_STATES.map(({ key, label }) => ({
			key,
			label,
			count: counts[key],
		})),
		{ key: "unhealthy" as const, label: "Unhealthy", count: counts.unhealthy },
	].filter((filter) => filter.count > 0);
	const matching = state === null ? counts.total : counts[state];
	const truncated = !query && visible.length < matching;

	return (
		<Card className="overflow-hidden">
			<div className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-gray-100">
				<h3 className="text-sm font-semibold text-gray-900 mr-1">
					Devices
					{wave !== null && (wave === 0 ? " in canary" : ` in wave ${wave}`)}
				</h3>
				{wave !== null && (
					<button
						type="button"
						onClick={onClearWave}
						className="text-xs text-blue-600 hover:underline cursor-pointer"
					>
						Show all
					</button>
				)}
				<div className="flex flex-wrap items-center gap-1">
					{filters.map((filter) => (
						<button
							key={filter.key}
							type="button"
							onClick={() => setState(state === filter.key ? null : filter.key)}
							className={`px-2 py-0.5 text-xs rounded-full border cursor-pointer ${
								state === filter.key
									? "border-gray-800 bg-gray-800 text-white"
									: "border-gray-200 text-gray-600 hover:bg-gray-50"
							}`}
						>
							{filter.label} {filter.count}
						</button>
					))}
				</div>
				<div className="relative ml-auto">
					<Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
					<input
						type="text"
						value={search}
						onChange={(e) => setSearch(e.target.value)}
						placeholder="Search serial"
						className="pl-8 pr-2 py-1 text-sm text-gray-900 border border-gray-300 rounded-md"
					/>
				</div>
			</div>
			<div className="overflow-x-auto max-h-[28rem] overflow-y-auto">
				<table className="min-w-full text-sm">
					<thead className="sticky top-0 z-10 bg-white">
						<tr className="text-left text-xs text-gray-500 border-b border-gray-100">
							<th className="px-4 py-2 font-medium">Serial</th>
							<th className="px-4 py-2 font-medium">Labels</th>
							<th className="px-4 py-2 font-medium">Wave</th>
							<th className="px-4 py-2 font-medium">Result</th>
							<th className="px-4 py-2 font-medium">From</th>
							<th className="px-4 py-2 font-medium">Now on</th>
							<th className="px-4 py-2 font-medium">Last seen</th>
						</tr>
					</thead>
					<tbody className="divide-y divide-gray-100">
						{visible.map((device) => {
							const info = deviceState(device.state);
							return (
								<tr key={device.device_id} className="hover:bg-gray-50">
									<td className="px-4 py-2 font-mono text-gray-900">
										<Link
											to={`/devices/${device.serial_number}`}
											className="hover:underline"
										>
											{device.serial_number}
										</Link>
									</td>
									<td className="px-4 py-2">
										<div className="flex flex-wrap gap-1.5 min-w-40">
											{Object.entries(device.labels ?? {})
												.sort(([a], [b]) => a.localeCompare(b))
												.map(([name, value]) => (
													<LabelChip key={name} name={name} value={value} />
												))}
											{Object.keys(device.labels ?? {}).length === 0 && (
												<span className="text-xs text-gray-400">—</span>
											)}
										</div>
									</td>
									<td className="px-4 py-2 text-gray-600">
										{device.wave === 0 ? "Canary" : device.wave}
									</td>
									<td className="px-4 py-2">
										<span className="inline-flex flex-wrap items-center gap-1">
											<Badge variant={info.variant}>
												{device.state === "scheduled"
													? `Waiting until ${formatClock(device.opens_at)}`
													: info.label}
											</Badge>
											{device.unhealthy_services.length > 0 && (
												<Badge variant="red">
													{device.unhealthy_services.join(", ")}
												</Badge>
											)}
										</span>
									</td>
									<td className="px-4 py-2 font-mono text-xs text-gray-500">
										{device.from_version ? `v${device.from_version}` : "-"}
									</td>
									<td className="px-4 py-2 font-mono text-xs text-gray-500">
										{device.current_version
											? `v${device.current_version}`
											: "-"}
									</td>
									<td className="px-4 py-2 text-gray-600 whitespace-nowrap">
										{device.last_ping ? (
											<RelativeTime date={device.last_ping} />
										) : (
											"Never"
										)}
									</td>
								</tr>
							);
						})}
						{visible.length === 0 && (
							<tr>
								<td colSpan={7} className="px-4 py-8 text-center text-gray-500">
									No devices match.
								</td>
							</tr>
						)}
					</tbody>
				</table>
			</div>
			{(truncated || visible.length === DEVICE_SAMPLE) && (
				<p className="px-4 py-2 border-t border-gray-100 text-xs text-gray-500">
					Showing the first {visible.length}
					{truncated ? ` of ${matching}` : ""} devices. Filter by state, wave or
					serial to find others.
				</p>
			)}
		</Card>
	);
};

type Dialog = "promote" | null;

const DeploymentPage = () => {
	const params = useParams();
	const deploymentId = Number(params.id);
	const queryClient = useQueryClient();
	const [dialog, setDialog] = useState<Dialog>(null);
	const [actionError, setActionError] = useState<string | null>(null);
	const [wave, setWave] = useState<number | null>(null);

	const {
		data: detail,
		isLoading,
		queryKey,
	} = useApiGetDeployment(deploymentId, { query: { refetchInterval: 5000 } });
	// Every filtered page of the device table.
	const devicesKey = getApiListDeploymentDevicesQueryKey(deploymentId);

	const mutation = {
		mutation: {
			onMutate: () => setActionError(null),
			onSuccess: () => {
				setDialog(null);
				queryClient.invalidateQueries({ queryKey });
				queryClient.invalidateQueries({ queryKey: devicesKey });
			},
			onError: (error: unknown) => {
				setDialog(null);
				setActionError(requestErrorMessage(error));
			},
		},
	};
	const promote = useApiPromoteDeployment(mutation);
	if (isLoading) {
		return (
			<PageContainer>
				<p className="text-sm text-gray-500">Loading...</p>
			</PageContainer>
		);
	}
	if (!detail) {
		return (
			<PageContainer>
				<BackLink to="/deployments">Deployments</BackLink>
				<p className="text-sm text-gray-500">Deployment not found.</p>
			</PageContainer>
		);
	}

	const { summary, actions, waves, events } = detail;
	const canaryWave = waves.find((w) => w.wave === 0);
	const rolloutWaves = waves.filter((w) => w.wave > 0);
	const paused = summary.state === "paused";
	const denied = actions.denied_reason ?? undefined;
	const pausedEvent = paused
		? events.find((event) => event.kind === "paused")
		: undefined;
	const toggleWave = (value: number) => {
		setWave(wave === value ? null : value);
		requestAnimationFrame(() =>
			document
				.getElementById("rollout-devices")
				?.scrollIntoView({ behavior: "smooth", block: "start" }),
		);
	};

	return (
		<PageContainer>
			<BackLink to="/deployments">Deployments</BackLink>

			<Card className="p-5 space-y-4">
				<div className="flex flex-wrap items-start justify-between gap-3">
					<div className="min-w-0">
						<div className="flex flex-wrap items-center gap-2">
							<ArchitectureIcon
								architecture={summary.distribution_architecture}
							/>
							<h1 className="text-xl font-semibold text-gray-900">
								<Link
									to={`/distributions/${summary.distribution_id}`}
									className="hover:underline"
								>
									{summary.distribution_name}
								</Link>
							</h1>
							<Link
								to={`/releases/${summary.release_id}`}
								className="font-mono text-gray-700 hover:underline"
							>
								{summary.base_version && `v${summary.base_version} → `}v
								{summary.release_version}
							</Link>
							<StateBadge state={summary.state} />
							<DeploymentBadges deployment={summary} />
						</div>
						<p className="mt-1 text-sm text-gray-500">
							Started by {summary.created_by ?? "unknown"}{" "}
							<RelativeTime date={summary.created_at} />
							{summary.completed_at && (
								<>
									{" "}
									· Finished <RelativeTime date={summary.completed_at} />
								</>
							)}
						</p>
					</div>
					<div className="flex flex-wrap items-center gap-2"></div>
				</div>

				<Steps detail={detail} />
				{summary.counts.total > 0 && (
					<div className="space-y-2">
						<ProgressBar counts={summary.counts} />
						<ProgressLegend counts={summary.counts} />
					</div>
				)}
				<p className="text-sm text-gray-700">{statusLine(summary)}</p>
				{pausedEvent && (
					<p className="text-sm text-yellow-800">
						Paused by {pausedEvent.user_email ?? "unknown"}{" "}
						<RelativeTime date={pausedEvent.created_at} />
						{pausedEvent.detail && `: ${pausedEvent.detail}`}
					</p>
				)}
			</Card>

			{actionError && (
				<AlertBanner title="That didn't work">{actionError}</AlertBanner>
			)}
			{denied && (
				<AlertBanner tone="amber" title="You can watch and pause this one">
					{denied}
				</AlertBanner>
			)}

			<div className="grid grid-cols-1 lg:grid-cols-3 gap-6 items-start">
				<div className="lg:col-span-2 space-y-6 min-w-0">
					{(canaryWave || rolloutWaves.length > 0) && (
						<Panel
							icon={Layers}
							title="Rollout"
							theme={SECTION_THEMES.blue}
							bodyClassName="p-0"
						>
							<div className="overflow-x-auto">
								<table
									className="min-w-full text-left"
									aria-label="Rollout progress"
								>
									<thead>
										<tr className="border-b border-gray-100 text-xs text-gray-500">
											<th className="px-4 py-2 font-medium">Group</th>
											<th className="px-4 py-2 font-medium">Schedule</th>
											<th className="px-4 py-2 font-medium">Progress</th>
											<th className="px-4 py-2 font-medium">Attention</th>
											<th className="px-4 py-2 font-medium text-right">
												Updated
											</th>
										</tr>
									</thead>
									{canaryWave && summary.kind === "staged" && (
										<tbody className="divide-y divide-gray-100">
											<tr className="bg-gray-50">
												<th
													colSpan={5}
													scope="rowgroup"
													className="px-4 py-2 text-xs font-semibold text-gray-600"
												>
													Canary
												</th>
											</tr>
											<WaveRow
												wave={canaryWave}
												paused={paused}
												selected={wave === 0}
												onSelect={() => toggleWave(0)}
											/>
											<tr>
												<td colSpan={5} className="p-4 space-y-3">
													{summary.promoted_at == null &&
													(summary.state === "canary" || paused) ? (
														<CanaryChecklist
															detail={detail}
															onPromote={() => setDialog("promote")}
														/>
													) : (
														<p className="text-sm text-gray-600">
															{canaryWave.counts.updated} of{" "}
															{canaryWave.counts.total} devices updated before
															the{" "}
															{summary.promoted_at == null
																? "deployment ended."
																: "rollout was confirmed."}
														</p>
													)}
												</td>
											</tr>
										</tbody>
									)}
									{rolloutWaves.length > 0 && (
										<tbody className="divide-y divide-gray-100">
											<tr className="bg-gray-50">
												<th
													colSpan={5}
													scope="rowgroup"
													className="px-4 py-2 text-xs font-semibold text-gray-600"
												>
													{summary.kind === "staged"
														? "Staggered rollout"
														: "Selected devices"}
												</th>
											</tr>
											{rolloutWaves.map((item) => (
												<WaveRow
													key={item.wave}
													wave={item}
													paused={paused}
													selected={wave === item.wave}
													onSelect={() => toggleWave(item.wave)}
												/>
											))}
										</tbody>
									)}
								</table>
							</div>
						</Panel>
					)}

					<div id="rollout-devices" className="scroll-mt-4">
						{" "}
						<DeviceTable
							deploymentId={deploymentId}
							counts={
								(wave !== null &&
									detail.waves.find((w) => w.wave === wave)?.counts) ||
								summary.counts
							}
							wave={wave}
							onClearWave={() => setWave(null)}
						/>
					</div>
				</div>

				<div className="space-y-6 min-w-0">
					<Panel
						icon={History}
						title="Activity"
						theme={SECTION_THEMES.gray}
						bodyClassName="p-4"
					>
						<Activity events={events} />
					</Panel>
				</div>
			</div>

			<Modal
				open={dialog === "promote"}
				onClose={() => setDialog(null)}
				title={`Roll v${summary.release_version} out to the fleet?`}
				footer={
					<>
						<Button tone="gray" onClick={() => setDialog(null)}>
							Not yet
						</Button>
						<Button
							variant="solid"
							tone="blue"
							loading={promote.isPending}
							disabled={!actions.can_promote}
							onClick={() => promote.mutate({ deploymentId })}
						>
							Confirm rollout
						</Button>
					</>
				}
			>
				<p className="text-sm text-gray-700">
					Eligible devices update in staggered waves after confirmation. Devices
					that do not follow latest stay on their current release.
				</p>
			</Modal>
		</PageContainer>
	);
};

export default DeploymentPage;
