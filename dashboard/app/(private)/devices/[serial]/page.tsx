import { AlertBanner, Card, PageContainer } from "@teton/smith-ui";
import { CheckCircle2, Tag, XCircle } from "lucide-react";
import { Link, useParams } from "react-router";
import {
	getCommandStatus,
	getTxLabel,
	parseRx,
	parseTx,
} from "@/app/(private)/commands/shared";
import {
	type Device,
	type DeviceCommandResponse,
	type Release,
	useGetAllCommandsForDevice,
	useGetDeviceInfo,
	useGetDistributionReleases,
} from "@/app/api-client";
import { RelativeTime } from "@/app/components/RelativeTime";
import { isStableRelease } from "@/app/utils/release";
import { DeviceDetailLayout } from "./DeviceDetailLayout";
import { getDeviceUpdateStatus } from "./DeviceHeader";
import { ReachabilityAlert, useReachabilityProblem } from "./uptime";

const COMMAND_PAGE_SIZE = 50;

// Escalates the banner from amber to red.
const CRITICAL_AFTER_MS = 7 * 24 * 60 * 60 * 1000;
const CRITICAL_ATTEMPTS = 3;
// Drift this large is its own problem, however long the device has been stuck:
// the build it runs is far enough back that nobody is testing against it.
const CRITICAL_BEHIND = 10;
const CRITICAL_RELEASE_AGE_MS = 180 * 24 * 60 * 60 * 1000;

/**
 * Tag chip for a release, linked to its page — same treatment as the device
 * header. Falls back to plain text when the release didn't come back with the
 * device and there is nothing to link to.
 *
 * `withDistribution` spells out `distribution@version`, used when the update
 * crosses distributions and the version alone would be ambiguous.
 */
const ReleaseLink = ({
	release,
	releaseId,
	withDistribution,
	className = "",
}: {
	release?: Release;
	releaseId?: number;
	withDistribution?: boolean;
	className?: string;
}) => {
	const version =
		release?.version ?? (releaseId != null ? `#${releaseId}` : null);
	if (version == null) return <span className={className}>unknown</span>;

	const label =
		withDistribution && release
			? `${release.distribution_name}@${version}`
			: `v${version}`;
	// Centres against its neighbours, like the header's chips — the caller lays
	// the line out as a flex row so nothing here depends on the text baseline.
	const body = (
		<>
			<Tag className="w-3.5 h-3.5 shrink-0" />
			<span>{label}</span>
		</>
	);

	if (!release) {
		return (
			<span className={`flex items-center gap-1 ${className}`}>{body}</span>
		);
	}

	return (
		<Link
			to={`/releases/${release.id}`}
			className={`flex items-center gap-1 rounded hover:text-blue-600 hover:underline transition-colors ${className}`}
		>
			{body}
		</Link>
	);
};

/** When a release was cut, so a version number reads as old or fresh. */
const ReleaseAge = ({ release }: { release?: Release }) =>
	release?.created_at ? (
		<span className="text-gray-400">
			(<RelativeTime date={release.created_at} />)
		</span>
	) : null;

/** First line of whatever the device sent back, for the "— reason" suffix. */
const failureReason = (cmd: DeviceCommandResponse): string | null => {
	const rx = parseRx(cmd.response);
	if (!rx) return null;
	if (rx.variant === "FreeForm") {
		const { stdout, stderr } = rx.payload as {
			stdout?: string;
			stderr?: string;
		};
		const text = (stderr || stdout || "").trim();
		return text ? text.split("\n")[0] : null;
	}
	return rx.variant;
};

/**
 * The overview's problem list: every check that fired, or a single all-clear
 * when none did — so the all-clear means the whole list is quiet, not just the
 * update check. Adding a check means adding its condition here.
 *
 * The update check is the long form of the header's "Update Failed" chip, gated
 * on the same rule so the two always agree. Nothing server-side records update
 * outcomes, so the attempt count is Upgrade commands that didn't move the
 * device, not failures it reported.
 */
const NeedsAttention = ({
	serial,
	device,
}: {
	serial: string;
	device: Device;
}) => {
	const update = getDeviceUpdateStatus(device);
	const failing = update?.status === "outdated";
	const { problem: unreachable, isLoading: checking } =
		useReachabilityProblem(serial);

	const { data: commands } = useGetAllCommandsForDevice(
		String(device.id),
		{ limit: COMMAND_PAGE_SIZE },
		{ query: { enabled: failing } },
	);

	// Only comparable within one distribution; a cross-distribution move has no
	// meaningful "behind by" count.
	const sameDistribution =
		device.release != null &&
		device.target_release != null &&
		device.release.distribution_id === device.target_release.distribution_id;

	const { data: releases } = useGetDistributionReleases(
		device.release?.distribution_id as number,
		{ query: { enabled: failing && sameDistribution } },
	);

	const targetSetAt = device.target_release_id_set_at
		? new Date(device.target_release_id_set_at).getTime()
		: null;

	// Attempts made for the current target only — earlier ones answered a
	// different target and say nothing about this one.
	const attempts = (commands?.commands ?? [])
		.filter(
			(cmd) =>
				!cmd.cancelled &&
				parseTx(cmd.cmd_data)?.variant === "Upgrade" &&
				(targetSetAt == null ||
					new Date(cmd.issued_at).getTime() >= targetSetAt),
		)
		.sort(
			(a, b) =>
				new Date(b.issued_at).getTime() - new Date(a.issued_at).getTime(),
		);

	// Versions alone are ambiguous when the target is on another distribution.
	const crossDistribution =
		device.release != null &&
		device.target_release != null &&
		!sameDistribution;

	// Stable releases cut after the one the device is on, up to and including its
	// target. Drafts and RCs are never deployed, so they aren't "behind".
	const behind =
		sameDistribution && device.release && device.target_release
			? (releases ?? []).filter((r) => {
					if (!isStableRelease(r)) return false;
					const cut = new Date(r.created_at).getTime();
					return (
						cut > new Date(device.release?.created_at ?? 0).getTime() &&
						cut <= new Date(device.target_release?.created_at ?? 0).getTime()
					);
				}).length
			: 0;

	const stuckFor = targetSetAt != null ? Date.now() - targetSetAt : 0;
	const runningAge = device.release?.created_at
		? Date.now() - new Date(device.release.created_at).getTime()
		: 0;
	// How far the running build has drifted, not just how long the update has
	// been failing — 62 releases back is serious even if it only just stalled.
	const staleBuild =
		behind >= CRITICAL_BEHIND || runningAge > CRITICAL_RELEASE_AGE_MS;
	const critical =
		attempts.length >= CRITICAL_ATTEMPTS ||
		stuckFor > CRITICAL_AFTER_MS ||
		staleBuild;
	const last = attempts[0];
	const reason = last ? failureReason(last) : null;

	if (!failing && !unreachable) {
		return (
			<div>
				<h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
					Needs attention
				</h3>
				{checking ? (
					<Card className="px-5 py-4 text-sm text-gray-400">
						Checking this device…
					</Card>
				) : (
					<Card className="px-5 py-4 flex items-center gap-2.5 text-sm text-gray-600">
						<CheckCircle2 className="w-4 h-4 shrink-0 text-green-500" />
						All good — nothing needs attention on this device.
					</Card>
				)}
			</div>
		);
	}

	return (
		<div>
			<h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
				Needs attention
			</h3>
			{/* Update first: a device on the wrong build is a fleet problem, a
			    flapping link is usually the site's. */}
			<div className="space-y-3">
				{failing && (
					<AlertBanner
						tone={critical ? "red" : "amber"}
						title={
							attempts.length > 0
								? `Update failed ${attempts.length} time${attempts.length === 1 ? "" : "s"}`
								: update?.duration
									? `Update stuck for ${update.duration}`
									: "Update stuck"
						}
					>
						<div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
							<span>Stuck on</span>
							<ReleaseLink
								release={device.release}
								releaseId={device.release_id}
								withDistribution={crossDistribution}
								className="text-gray-700"
							/>
							<ReleaseAge release={device.release} />
							<span className="text-gray-400">→</span>
							<ReleaseLink
								release={device.target_release}
								releaseId={device.target_release_id}
								withDistribution={crossDistribution}
								className="text-gray-700"
							/>
							<ReleaseAge release={device.target_release} />
							{behind > 0 && (
								<span
									className={
										staleBuild ? "font-medium text-red-600" : "text-gray-500"
									}
								>
									· {behind} release{behind === 1 ? "" : "s"} behind
								</span>
							)}
						</div>
						{last && (
							<div className="mt-1">
								Last attempt <RelativeTime date={last.issued_at} />
								{reason && (
									<>
										{" — "}
										<span className="font-mono">{reason}</span>
									</>
								)}
							</div>
						)}
					</AlertBanner>
				)}
				{unreachable && <ReachabilityAlert summary={unreachable} />}
			</div>
		</div>
	);
};

const STATUS_DOT: Record<string, string> = {
	success: "bg-green-500",
	failed: "bg-red-500",
	executing: "bg-blue-500",
	pending: "bg-yellow-500",
	cancelled: "bg-gray-300",
};

const RECENT_COMMANDS = 5;

/** Last few commands sent to the device, as a digest of the Commands tab. */
const RecentActivity = ({
	serial,
	device,
}: {
	serial: string;
	device: Device;
}) => {
	const { data: commands, isLoading } = useGetAllCommandsForDevice(
		String(device.id),
		{ limit: COMMAND_PAGE_SIZE },
	);

	const recent = (commands?.commands ?? [])
		.slice()
		.sort(
			(a, b) =>
				new Date(b.issued_at).getTime() - new Date(a.issued_at).getTime(),
		)
		.slice(0, RECENT_COMMANDS);

	return (
		<div>
			<div className="flex items-center justify-between gap-2 mb-2">
				<h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500">
					Recent activity
				</h3>
				<Link
					to={`/devices/${serial}/commands`}
					className="text-sm text-blue-600 hover:text-blue-700 transition-colors"
				>
					All commands
				</Link>
			</div>

			<Card className="overflow-hidden">
				{isLoading ? (
					<div className="px-4 py-6 text-gray-500">Loading commands...</div>
				) : recent.length === 0 ? (
					<div className="px-4 py-6 text-gray-500 text-sm">
						No commands sent to this device yet.
					</div>
				) : (
					<div className="divide-y divide-gray-100">
						{recent.map((cmd) => {
							const status = getCommandStatus(cmd);
							const { label, mono } = getTxLabel(cmd.cmd_data);
							// Exit code is only meaningful once the device answered.
							const exit =
								cmd.response != null && cmd.status != null ? cmd.status : null;

							return (
								<div key={cmd.cmd_id} className="flex gap-2.5 px-4 py-3">
									<span
										className={`mt-1.5 h-2 w-2 rounded-full shrink-0 ${STATUS_DOT[status] ?? "bg-gray-300"}`}
									/>
									<div className="min-w-0">
										<p
											className={`text-sm font-medium text-gray-900 truncate ${mono ? "font-mono" : ""}`}
											title={label}
										>
											{label}
										</p>
										<div className="flex flex-wrap items-center gap-x-1.5 font-mono text-xs text-gray-400">
											<span>{status}</span>
											{exit != null && (
												<>
													<span>·</span>
													<span>exit {exit}</span>
												</>
											)}
											<span>·</span>
											<RelativeTime date={cmd.issued_at} />
											<span>·</span>
											<span className="truncate">
												by {cmd.user_email ?? "system"}
											</span>
										</div>
									</div>
								</div>
							);
						})}
					</div>
				)}
			</Card>
		</div>
	);
};

const DeviceDetailPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const { data: device, isLoading: loading } = useGetDeviceInfo(serial);

	if (loading) {
		return (
			<PageContainer>
				<div className="h-4 w-40 bg-gray-200 rounded animate-pulse" />
				<Card className="p-6">
					<div className="flex items-center space-x-4">
						<div className="p-3 bg-gray-100 rounded-lg">
							<div className="w-8 h-8 bg-gray-200 rounded animate-pulse" />
						</div>
						<div className="space-y-2">
							<div className="h-8 bg-gray-200 rounded w-48 animate-pulse" />
							<div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
							<div className="h-4 bg-gray-200 rounded w-24 animate-pulse" />
						</div>
					</div>
				</Card>
			</PageContainer>
		);
	}

	if (!device) {
		return (
			<PageContainer>
				<div className="text-center py-12">
					<XCircle className="w-12 h-12 text-gray-400 mx-auto mb-4" />
					<h3 className="text-lg font-medium text-gray-900 mb-2">
						Device not found
					</h3>
					<p className="text-gray-500">
						The device with serial number "{serial}" could not be found.
					</p>
				</div>
			</PageContainer>
		);
	}

	/* Landing tab. Its detail sections moved out to the Network and System tabs,
	   so it carries problems on the left and a digest of recent commands on the
	   right. */
	return (
		<DeviceDetailLayout serial={serial} device={device} activeTab="overview">
			<div className="grid grid-cols-1 lg:grid-cols-3 gap-4 items-start">
				<div className="lg:col-span-2 space-y-4">
					<NeedsAttention serial={serial} device={device} />
				</div>
				<RecentActivity serial={serial} device={device} />
			</div>
		</DeviceDetailLayout>
	);
};

export default DeviceDetailPage;
