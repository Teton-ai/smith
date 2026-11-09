import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, type BadgeVariant, Button, Card } from "@teton/smith-ui";
import {
	ArrowDownLeft,
	ArrowRightLeft,
	ArrowUpRight,
	Clock,
	GitBranch,
	History,
	Minus,
	Package,
	Pin,
	PinOff,
	Plus,
	RefreshCw,
	Tag,
} from "lucide-react";
import { Link } from "react-router";
import { type Device, getGetDeviceInfoQueryKey } from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { RelativeTime } from "@/app/components/RelativeTime";
import { versionChangeKind } from "@/app/utils/versionChange";
import { getDeviceUpdateStatus } from "./DeviceHeader";

interface PackageChange {
	name: string;
	architecture: string;
	previous_package_id: number | null;
	previous_version: string | null;
	package_id: number | null;
	version: string | null;
}

interface VersionChange {
	id: number;
	observed_at: string;
	package_changes?: PackageChange[];
	previous_release_id: number;
	previous_version: string;
	previous_distribution: string;
	release_id: number;
	version: string;
	distribution: string;
}

interface PinEvent {
	id: number;
	follow_latest: boolean;
	changed_at: string;
}

type HistoryEvent =
	| { kind: "version"; data: VersionChange }
	| { kind: "pin"; data: PinEvent };
interface DeviceHistoryPage {
	events: HistoryEvent[];
	next_cursor: string | null;
}

const changeStyles = {
	upgrade: {
		label: "Upgrade",
		icon: ArrowUpRight,
		variant: "green",
		row: "border-l-green-500 bg-green-50/40",
		tile: "bg-green-100 text-green-700",
		version: "text-green-800",
	},
	rollback: {
		label: "Downgrade",
		icon: ArrowDownLeft,
		variant: "orange",
		row: "border-l-amber-500 bg-amber-50/50",
		tile: "bg-amber-100 text-amber-700",
		version: "text-amber-800",
	},
	distribution: {
		label: "Distribution change",
		icon: GitBranch,
		variant: "purple",
		row: "border-l-purple-500 bg-purple-50/40",
		tile: "bg-purple-100 text-purple-700",
		version: "text-purple-800",
	},
	change: {
		label: "Release change",
		icon: ArrowRightLeft,
		variant: "gray",
		row: "border-l-gray-300",
		tile: "bg-gray-100 text-gray-600",
		version: "text-gray-900",
	},
} satisfies Record<
	ReturnType<typeof versionChangeKind>,
	{
		label: string;
		icon: typeof History;
		variant: BadgeVariant;
		row: string;
		tile: string;
		version: string;
	}
>;

function VersionLink({
	id,
	version,
	distribution,
	showDistribution = false,
}: {
	id: number;
	version: string;
	distribution: string;
	showDistribution?: boolean;
}) {
	return (
		<Link
			to={`/releases/${id}`}
			title={`${distribution}@${version}`}
			className="inline-flex min-w-0 max-w-full items-center gap-1.5 hover:underline"
		>
			<Tag className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
			<span className="min-w-0 break-all">
				{showDistribution ? `${distribution}@${version}` : `v${version}`}
			</span>
		</Link>
	);
}

function PackageDelta({ change }: { change: PackageChange }) {
	const added = change.previous_package_id == null;
	const removed = change.package_id == null;
	const replaced =
		!added && !removed && change.previous_version === change.version;
	const Icon = added ? Plus : removed ? Minus : Package;
	return (
		<li className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-xs">
			<span
				className="inline-flex items-center gap-1.5 font-medium text-gray-700"
				title={change.architecture}
			>
				<Icon
					className={`h-3 w-3 shrink-0 self-center ${added ? "text-green-600" : removed ? "text-red-500" : "text-gray-400"}`}
					aria-hidden="true"
				/>
				{change.name}
			</span>
			{added || removed || replaced ? (
				<>
					<span className="font-mono text-gray-600">
						{change.version ?? change.previous_version}
					</span>
					<Badge variant={added ? "green" : removed ? "red" : "gray"}>
						{added ? "Added" : removed ? "Removed" : "Replaced"}
					</Badge>
				</>
			) : (
				<span className="inline-flex flex-wrap items-baseline gap-1.5 font-mono">
					<span className="text-gray-400">{change.previous_version}</span>
					<span className="sr-only">changed to</span>
					<span className="text-gray-400" aria-hidden="true">
						→
					</span>
					<span className="font-semibold text-gray-800">{change.version}</span>
				</span>
			)}
		</li>
	);
}

function PackageChanges({ changes }: { changes?: PackageChange[] }) {
	if (!changes)
		return (
			<p className="mt-2 text-xs text-gray-500">Package details unavailable.</p>
		);
	if (changes.length === 0)
		return (
			<p className="mt-2 text-xs text-gray-500">No package changes recorded.</p>
		);
	return (
		<div className="mt-2.5 space-y-1.5">
			<p className="text-xs font-medium text-gray-500">
				Release packages · {changes.length} changed
			</p>
			<ul className="space-y-1.5">
				{changes.map((change) => (
					<PackageDelta
						key={`${change.previous_package_id}:${change.package_id}`}
						change={change}
					/>
				))}
			</ul>
		</div>
	);
}

function PinHistoryEvent({ event }: { event: PinEvent }) {
	const Icon = event.follow_latest ? PinOff : Pin;
	return (
		<li
			className={`flex items-start gap-3 border-l-2 px-4 py-3 ${event.follow_latest ? "border-l-blue-500 bg-blue-50/40" : "border-l-purple-500 bg-purple-50/40"}`}
		>
			<span
				className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg ${event.follow_latest ? "bg-blue-100 text-blue-700" : "bg-purple-100 text-purple-700"}`}
			>
				<Icon className="h-4 w-4" aria-hidden="true" />
			</span>
			<div className="grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_8rem] items-start gap-x-3">
				<div className="space-y-0.5">
					<p
						className={`text-sm font-semibold ${event.follow_latest ? "text-blue-800" : "text-purple-800"}`}
					>
						{event.follow_latest ? "Unpinned" : "Pinned"}
					</p>
					<p className="text-xs text-gray-500">
						{event.follow_latest
							? "Resumed following latest"
							: "Stopped following latest"}
					</p>
				</div>
				<time
					dateTime={event.changed_at}
					title={new Date(event.changed_at).toLocaleString()}
					className="text-right text-xs text-gray-500"
				>
					<RelativeTime date={event.changed_at} />
				</time>
			</div>
		</li>
	);
}

export function DeviceHistory({ device }: { device: Device }) {
	const fetcher = useClientMutator<DeviceHistoryPage>();
	const queryClient = useQueryClient();
	const history = useInfiniteQuery({
		queryKey: [`/devices/${device.id}/history`, device.follow_latest],
		initialPageParam: undefined as string | undefined,
		queryFn: ({ pageParam, signal }) =>
			fetcher({
				url: `/devices/${device.id}/history`,
				method: "GET",
				params: { before: pageParam, limit: 5 },
				signal,
			}),
		getNextPageParam: (page) => page.next_cursor ?? undefined,
	});
	const events = history.data?.pages.flatMap((page) => page.events) ?? [];
	const update = getDeviceUpdateStatus(device);
	const pending = update !== null;
	const scheduled = update?.status === "scheduled";

	return (
		<section aria-labelledby="device-history-heading">
			<div className="flex items-center justify-between gap-2 mb-2">
				<h3
					id="device-history-heading"
					className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-gray-500"
				>
					<History className="w-4 h-4 text-blue-500" aria-hidden="true" />
					Device history
				</h3>
				<Button
					variant="soft"
					tone="gray"
					size="sm"
					icon={<RefreshCw className="w-3.5 h-3.5" aria-hidden="true" />}
					loading={history.isFetching}
					disabled={history.isFetching}
					onClick={() => {
						history.refetch();
						queryClient.invalidateQueries({
							queryKey: getGetDeviceInfoQueryKey(device.serial_number),
							exact: true,
						});
					}}
				>
					Refresh
				</Button>
			</div>
			<Card className="overflow-hidden">
				<div className="px-4 py-4 space-y-3 border-b border-gray-100">
					<div className="flex flex-wrap items-center gap-2 text-sm">
						<Package className="w-4 h-4 text-blue-500" aria-hidden="true" />
						<Badge variant={device.follow_latest ? "blue" : "purple"}>
							{device.follow_latest ? "Following latest" : "Pinned"}
						</Badge>
						<span className="text-gray-500">Last reported</span>
						{device.release ? (
							<VersionLink
								id={device.release.id}
								version={device.release.version}
								distribution={device.release.distribution_name}
								showDistribution={
									device.target_release != null &&
									device.release.distribution_id !==
										device.target_release.distribution_id
								}
							/>
						) : (
							<span className="text-gray-500">Unknown</span>
						)}
						{!pending &&
							device.release_id != null &&
							device.target_release_id === device.release_id && (
								<Badge variant="green">On target</Badge>
							)}
					</div>
					{pending && (
						<div className="space-y-1.5 text-sm">
							<div className="flex flex-wrap items-center gap-2">
								<Clock className="w-4 h-4 text-amber-500" aria-hidden="true" />
								<Badge variant={scheduled ? "purple" : "orange"}>
									{scheduled
										? "Scheduled"
										: update?.status === "outdated"
											? "Overdue"
											: "Pending"}
								</Badge>
								{device.target_release ? (
									<VersionLink
										id={device.target_release.id}
										version={device.target_release.version}
										distribution={device.target_release.distribution_name}
										showDistribution={
											device.release == null ||
											device.release.distribution_id !==
												device.target_release.distribution_id
										}
									/>
								) : (
									<span>Release #{device.target_release_id}</span>
								)}
							</div>
							<p className="text-xs text-gray-500">
								{update?.status === "scheduled"
									? `Eligible ${update.eligibleAt.toLocaleString()}. `
									: ""}
								{device.release_id == null
									? "No installed version has been reported yet."
									: "Target differs from the last reported release."}{" "}
								Update progress is not reported by the device.
							</p>
						</div>
					)}
					<p className="text-xs text-gray-500">
						Release and pin changes, newest first.
					</p>
				</div>
				{history.isPending ? (
					<output className="block px-4 py-6 text-sm text-gray-500">
						Loading device history…
					</output>
				) : history.isError && !history.data ? (
					<div role="alert" className="px-4 py-5 space-y-3 text-sm">
						<p className="text-red-600">Could not load device history.</p>
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							onClick={() => history.refetch()}
						>
							Retry
						</Button>
					</div>
				) : events.length === 0 ? (
					<p className="px-4 py-6 text-sm text-gray-500">
						No release or pin changes recorded for this device.
					</p>
				) : (
					<ol
						aria-label="Device history, newest first"
						className="divide-y divide-gray-100"
					>
						{events.map((event) => {
							if (event.kind === "pin")
								return (
									<PinHistoryEvent
										key={`pin-${event.data.id}`}
										event={event.data}
									/>
								);
							const change = event.data;
							const distributionChanged =
								change.previous_distribution !== change.distribution;
							const style =
								changeStyles[
									versionChangeKind(
										change.previous_version,
										change.version,
										distributionChanged,
									)
								];
							const Icon = style.icon;
							return (
								<li
									key={`version-${change.id}`}
									className={`flex items-start gap-3 border-l-2 px-4 py-3 ${style.row}`}
								>
									<span
										className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg ${style.tile}`}
									>
										<Icon className="w-4 h-4" aria-hidden="true" />
									</span>
									<div className="min-w-0 flex-1">
										<div className="grid grid-cols-[minmax(0,1fr)_8rem] items-start gap-x-3">
											<div className="min-w-0 space-y-0.5">
												<div
													className={`text-lg font-semibold leading-snug tracking-tight ${style.version}`}
												>
													<VersionLink
														id={change.release_id}
														version={change.version}
														distribution={change.distribution}
														showDistribution={distributionChanged}
													/>
												</div>
												<div className="flex items-center gap-1.5 text-xs text-gray-400">
													<span className="shrink-0">From</span>
													<VersionLink
														id={change.previous_release_id}
														version={change.previous_version}
														distribution={change.previous_distribution}
														showDistribution={distributionChanged}
													/>
												</div>
											</div>
											<div className="flex flex-col items-end gap-1.5 text-right">
												<Badge variant={style.variant}>{style.label}</Badge>
												<time
													dateTime={change.observed_at}
													title={`Reported ${new Date(change.observed_at).toLocaleString()}`}
													className="text-xs text-gray-500"
												>
													<RelativeTime date={change.observed_at} />
												</time>
											</div>
										</div>
										<PackageChanges changes={change.package_changes} />
									</div>
								</li>
							);
						})}
					</ol>
				)}
				{history.isError && history.data && (
					<p role="alert" className="px-4 py-3 text-sm text-red-600">
						{history.isFetchNextPageError
							? "Could not load older changes. Try again below."
							: "Could not refresh history. Displayed entries may be out of date."}
					</p>
				)}
				{events.length > 0 && (
					<div className="border-t border-gray-100 px-4 py-3">
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							aria-busy={history.isFetchingNextPage}
							aria-disabled={history.isFetching || !history.hasNextPage}
							onClick={() => {
								if (!history.isFetching && history.hasNextPage)
									history.fetchNextPage();
							}}
						>
							{history.isFetchingNextPage
								? "Loading older changes…"
								: !history.hasNextPage
									? "All recorded changes loaded"
									: history.isFetchNextPageError
										? "Retry loading older changes"
										: "Load older changes"}
						</Button>
					</div>
				)}
				<output className="sr-only">
					{history.isFetching
						? "Loading device history"
						: `${events.length} history events loaded`}
				</output>
			</Card>
		</section>
	);
}
