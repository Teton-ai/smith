import {
	Button,
	Card,
	PageContainer,
	SelectMenu,
	StatCard,
} from "@teton/smith-ui";
import { CheckCircle2, Clock, Rocket, XCircle } from "lucide-react";
import { Link, useSearchParams } from "react-router";
import {
	type ApiListDeploymentsParams,
	type DeploymentKind,
	type DeploymentSummary,
	useApiListDeploymentCreators,
	useApiListDeployments,
	useGetDistributions,
} from "@/app/api-client";
import { ArchitectureIcon } from "@/app/components/ArchitectureIcon";
import { RelativeTime } from "@/app/components/RelativeTime";
import { DeploymentBadges, ProgressBar, StateBadge } from "./shared";

function Row({
	deployment,
	history = false,
}: {
	deployment: DeploymentSummary;
	/** In the history only failures are worth a badge: deployed is the norm. */
	history?: boolean;
}) {
	return (
		<Link
			to={`/deployments/${deployment.id}`}
			className="grid grid-cols-12 items-center gap-x-4 gap-y-2 px-4 py-3 hover:bg-gray-50 transition-colors"
		>
			<div className="col-span-8 md:col-span-4 min-w-0">
				<div className="flex flex-wrap items-center gap-2">
					<span className="text-xs text-gray-400">#{deployment.id}</span>
					<span className="font-medium text-gray-900">
						Release{" "}
						<span className="font-mono">v{deployment.release_version}</span>
					</span>
					<DeploymentBadges deployment={deployment} />
				</div>
				<div className="truncate text-xs text-gray-500">
					{deployment.kind === "rollback" && deployment.rolled_back_from_version
						? `v${deployment.rolled_back_from_version} → v${deployment.release_version}`
						: deployment.base_version
							? `v${deployment.base_version} → v${deployment.release_version}`
							: "Initial deployment"}
				</div>
			</div>
			<div className="col-span-4 md:col-span-2 flex min-w-0 items-center gap-2 text-sm text-gray-700">
				<ArchitectureIcon
					architecture={deployment.distribution_architecture}
					size="sm"
				/>
				<span className="truncate">{deployment.distribution_name}</span>
			</div>
			<div className="col-span-6 md:col-span-3 space-y-1">
				{deployment.counts.total > 0 ? (
					<>
						<ProgressBar counts={deployment.counts} />
						<div className="text-xs text-gray-500">
							{deployment.counts.updated}/{deployment.counts.total} devices
							updated
						</div>
					</>
				) : (
					<span className="text-xs text-gray-400">Not tracked</span>
				)}
			</div>
			<div className="col-span-3 md:col-span-1">
				{(!history ||
					deployment.state === "canceled" ||
					deployment.state === "failed") && (
					<StateBadge state={deployment.state} />
				)}
			</div>
			<div className="col-span-3 md:col-span-2 min-w-0 text-right text-xs text-gray-500">
				<div
					className="truncate text-gray-800"
					title={deployment.created_by ?? undefined}
				>
					{deployment.created_by ?? "Unknown"}
				</div>
				<RelativeTime date={deployment.created_at} />
			</div>
		</Link>
	);
}

const KIND_OPTIONS: { value: DeploymentKind | ""; label: string }[] = [
	{ value: "", label: "All types" },
	{ value: "staged", label: "Staged" },
	{ value: "direct", label: "Direct" },
	{ value: "rollback", label: "Rollbacks" },
];

const OUTCOME_OPTIONS = [
	{ value: "", label: "All outcomes" },
	{ value: "completed", label: "Deployed" },
	{ value: "canceled", label: "Canceled" },
	{ value: "failed", label: "Failed" },
];

const FILTERS = ["distribution", "user", "kind", "outcome"] as const;

export default function DeploymentsPage() {
	const [params, setParams] = useSearchParams();
	const distributionId = params.get("distribution") ?? "";
	const userId = params.get("user") ?? "";
	const kind = (params.get("kind") ?? "") as DeploymentKind | "";
	const outcome = params.get("outcome") ?? "";
	const filtered = FILTERS.some((name) => params.get(name));

	const { data: distributions = [] } = useGetDistributions();
	const { data: creators = [] } = useApiListDeploymentCreators();
	// Active rollouts are always shown in full, however old; the filters narrow the
	// history. Both are filtered by the API, so the limit can't hide an active one.
	const {
		data: active = [],
		isLoading,
		isError,
	} = useApiListDeployments(
		{ active: true, limit: 200 },
		{ query: { refetchInterval: 5000 } },
	);
	const historyParams: ApiListDeploymentsParams = {
		active: false,
		limit: 200,
		...(distributionId ? { distribution_id: Number(distributionId) } : {}),
		...(userId ? { created_by: Number(userId) } : {}),
		...(kind ? { kind } : {}),
	};
	const {
		data: past = [],
		isLoading: historyLoading,
		isError: historyError,
	} = useApiListDeployments(historyParams, {
		query: { refetchInterval: 10000 },
	});
	const history = past.filter((d) => !outcome || d.state === outcome);
	const setFilter = (name: (typeof FILTERS)[number], value: string) => {
		setParams((previous) => {
			const next = new URLSearchParams(previous);
			if (value) next.set(name, value);
			else next.delete(name);
			return next;
		});
	};
	const clearFilters = () => {
		setParams((previous) => {
			const next = new URLSearchParams(previous);
			for (const name of FILTERS) next.delete(name);
			return next;
		});
	};

	return (
		<PageContainer className="space-y-6 text-gray-900">
			<div className="flex items-center justify-between">
				<div>
					<h1 className="text-2xl font-bold text-gray-900">Deployments</h1>
					<p className="text-sm text-gray-500">
						A deployment rolls a published release out to a distribution:
						canaries first, then waves. To start one, open a{" "}
						<Link to="/distributions" className="text-blue-600 hover:underline">
							distribution
						</Link>
						, draft a release, publish it and deploy it.
					</p>
				</div>
			</div>

			<div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
				<Card className="p-5">
					<StatCard
						icon={Rocket}
						label="Active"
						value={active.length}
						tone="blue"
					/>
				</Card>
				<Card className="p-5">
					<StatCard
						icon={Clock}
						label="Paused"
						value={active.filter((d) => d.state === "paused").length}
						tone="orange"
					/>
				</Card>
				<Card className="p-5">
					<StatCard
						icon={CheckCircle2}
						label="Deployed"
						value={past.filter((d) => d.state === "completed").length}
						tone="green"
					/>
				</Card>
				<Card className="p-5">
					<StatCard
						icon={XCircle}
						label="Canceled"
						value={past.filter((d) => d.state === "canceled").length}
						tone="red"
					/>
				</Card>
			</div>

			<Card className="overflow-hidden">
				<div className="px-4 py-3 border-b border-gray-100 text-sm font-semibold text-gray-700">
					Active
				</div>
				<div className="divide-y divide-gray-100">
					{isLoading ? (
						<p className="p-8 text-center text-sm text-gray-400">
							Loading deployments…
						</p>
					) : isError ? (
						<p role="alert" className="p-8 text-center text-sm text-red-600">
							Could not load deployments.
						</p>
					) : active.length === 0 ? (
						<div className="p-8 text-center text-sm text-gray-400">
							Nothing rolling out right now.
						</div>
					) : (
						active.map((d) => <Row key={d.id} deployment={d} />)
					)}
				</div>
			</Card>

			<Card className="overflow-hidden">
				<div className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-gray-100">
					<span className="mr-2 text-sm font-semibold text-gray-700">
						History
					</span>
					<SelectMenu
						ariaLabel="Distribution"
						searchable
						searchPlaceholder="Search distributions"
						value={distributionId}
						onChange={(value) => setFilter("distribution", value)}
						menuClassName="w-72"
						options={[
							{ value: "", label: "All distributions" },
							...[...distributions]
								.sort((a, b) => a.name.localeCompare(b.name))
								.map((dist) => ({ value: String(dist.id), label: dist.name })),
						]}
					/>
					<SelectMenu
						ariaLabel="User"
						searchable
						searchPlaceholder="Search users"
						value={userId}
						onChange={(value) => setFilter("user", value)}
						menuClassName="w-72"
						options={[
							{ value: "", label: "All users" },
							...creators.map((creator) => ({
								value: String(creator.id),
								label: creator.email ?? `User #${creator.id}`,
							})),
						]}
					/>
					<SelectMenu
						ariaLabel="Type"
						value={kind}
						onChange={(value) => setFilter("kind", value)}
						options={KIND_OPTIONS}
					/>
					<SelectMenu
						ariaLabel="Outcome"
						value={outcome}
						onChange={(value) => setFilter("outcome", value)}
						options={OUTCOME_OPTIONS}
					/>
					{filtered && (
						<Button
							size="sm"
							variant="ghost"
							tone="gray"
							onClick={clearFilters}
						>
							Clear filters
						</Button>
					)}
					<output className="ml-auto text-sm text-gray-500">
						{history.length} deployment{history.length === 1 ? "" : "s"}
					</output>
				</div>
				<div className="divide-y divide-gray-100">
					{historyLoading ? (
						<p className="p-8 text-center text-sm text-gray-400">
							Loading history…
						</p>
					) : historyError ? (
						<p className="p-8 text-center text-sm text-gray-400">
							History is unavailable.
						</p>
					) : history.length === 0 ? (
						<div className="p-8 text-center text-sm text-gray-400">
							{filtered
								? "No past deployments match these filters."
								: "No past deployments yet."}
						</div>
					) : (
						history.map((d) => <Row key={d.id} deployment={d} history />)
					)}
				</div>
			</Card>
		</PageContainer>
	);
}
