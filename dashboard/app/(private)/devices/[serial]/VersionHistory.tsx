import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, Button, Card } from "@teton/smith-ui";
import {
	ArrowRight,
	Clock,
	History,
	Package,
	RefreshCw,
	Tag,
} from "lucide-react";
import { Link } from "react-router";
import { type Device, getGetDeviceInfoQueryKey } from "@/app/api-client";
import { useClientMutator } from "@/app/api-client-mutator";
import { RelativeTime } from "@/app/components/RelativeTime";
import { getDeviceUpdateStatus } from "./DeviceHeader";

interface VersionChange {
	id: number;
	observed_at: string;
	previous_release_id: number;
	previous_version: string;
	previous_distribution: string;
	release_id: number;
	version: string;
	distribution: string;
}

interface VersionHistoryPage {
	changes: VersionChange[];
	next_cursor: number | null;
}

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
			className="inline-flex items-center gap-1.5 text-blue-600 hover:underline break-all"
		>
			<Tag className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
			{showDistribution ? `${distribution}@${version}` : `v${version}`}
		</Link>
	);
}

export function VersionHistory({ device }: { device: Device }) {
	const fetcher = useClientMutator<VersionHistoryPage>();
	const queryClient = useQueryClient();
	const history = useInfiniteQuery({
		queryKey: [`/devices/${device.id}/version-history`],
		initialPageParam: undefined as number | undefined,
		queryFn: ({ pageParam, signal }) =>
			fetcher({
				url: `/devices/${device.id}/version-history`,
				method: "GET",
				params: { before: pageParam, limit: 5 },
				signal,
			}),
		getNextPageParam: (page) => page.next_cursor ?? undefined,
	});
	const changes = history.data?.pages.flatMap((page) => page.changes) ?? [];
	const update = getDeviceUpdateStatus(device);
	const pending = update !== null;
	const scheduled = update?.status === "scheduled";

	return (
		<section aria-labelledby="version-history-heading">
			<div className="flex items-center justify-between gap-2 mb-2">
				<h3
					id="version-history-heading"
					className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-gray-500"
				>
					<History className="w-4 h-4 text-blue-500" aria-hidden="true" />
					Version history
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
						Times show when Smith observed a version change, not when
						installation finished. Only reported version changes are shown.
						Versions installed between check-ins may be missing.
					</p>
				</div>
				{history.isPending ? (
					<output className="block px-4 py-6 text-sm text-gray-500">
						Loading version history…
					</output>
				) : history.isError && !history.data ? (
					<div role="alert" className="px-4 py-5 space-y-3 text-sm">
						<p className="text-red-600">Could not load version history.</p>
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							onClick={() => history.refetch()}
						>
							Retry
						</Button>
					</div>
				) : changes.length === 0 ? (
					<p className="px-4 py-6 text-sm text-gray-500">
						No version changes recorded for this device.
					</p>
				) : (
					<ol
						aria-label="Observed version changes, newest first"
						className="divide-y divide-gray-100"
					>
						{changes.map((change) => (
							<li key={change.id} className="flex gap-3 px-4 py-4">
								<span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-blue-50 text-blue-600">
									<History className="w-4 h-4" aria-hidden="true" />
								</span>
								<div className="min-w-0 space-y-1.5">
									<div className="flex flex-wrap items-center gap-2 text-sm">
										<VersionLink
											id={change.previous_release_id}
											version={change.previous_version}
											distribution={change.previous_distribution}
											showDistribution={
												change.previous_distribution !== change.distribution
											}
										/>
										<span className="sr-only">changed to</span>
										<ArrowRight
											className="w-4 h-4 shrink-0 text-gray-400"
											aria-hidden="true"
										/>
										<VersionLink
											id={change.release_id}
											version={change.version}
											distribution={change.distribution}
											showDistribution={
												change.previous_distribution !== change.distribution
											}
										/>
									</div>
									<div className="flex flex-wrap items-center gap-x-2 text-xs text-gray-500">
										<RelativeTime date={change.observed_at} />
										<time dateTime={change.observed_at}>
											{new Date(change.observed_at).toLocaleString()}
										</time>
									</div>
								</div>
							</li>
						))}
					</ol>
				)}
				{history.isError && history.data && (
					<p role="alert" className="px-4 py-3 text-sm text-red-600">
						{history.isFetchNextPageError
							? "Could not load older changes. Try again below."
							: "Could not refresh history. Displayed entries may be out of date."}
					</p>
				)}
				{changes.length > 0 && (
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
						? "Loading version history"
						: `${changes.length} version changes loaded`}
				</output>
			</Card>
		</section>
	);
}
