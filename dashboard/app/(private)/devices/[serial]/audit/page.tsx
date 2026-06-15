import {
	ArrowLeft,
	CheckCircle2,
	MinusCircle,
	RefreshCw,
	XCircle,
} from "lucide-react";
import { Link, useNavigate, useParams } from "react-router";
import { useGetDeviceInfo, useIssueCommandsToDevices } from "@/app/api-client";
import { Button } from "@/app/components/button";
import DeviceHeader from "../DeviceHeader";
import { type DeviceAudit, useDeviceAudit } from "./useDeviceAudit";

/** Renders a yes / no / unknown status pill for a single audit check. */
const StatusPill = ({ value }: { value: boolean | null }) => {
	if (value === null || value === undefined) {
		return (
			<span className="inline-flex items-center gap-1.5 text-gray-400 text-sm font-medium">
				<MinusCircle className="w-4 h-4" />
				Unknown
			</span>
		);
	}
	return value ? (
		<span className="inline-flex items-center gap-1.5 text-green-600 text-sm font-medium">
			<CheckCircle2 className="w-4 h-4" />
			Yes
		</span>
	) : (
		<span className="inline-flex items-center gap-1.5 text-red-600 text-sm font-medium">
			<XCircle className="w-4 h-4" />
			No
		</span>
	);
};

const AuditPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const navigate = useNavigate();

	const { data: device, isLoading: deviceLoading } = useGetDeviceInfo(serial);
	const {
		data: audit,
		isLoading: auditLoading,
		refetch,
	} = useDeviceAudit(serial);

	const { mutate: runAudit, isPending: isRunningAudit } =
		useIssueCommandsToDevices({
			mutation: {
				onSuccess: () => {
					// The device reports back asynchronously; refetch shortly after
					// to pick up the new result once it lands.
					setTimeout(() => refetch(), 3000);
				},
				onError: (error) => {
					console.error("Failed to trigger audit:", error);
				},
			},
		});

	const handleRunAudit = () => {
		if (!device?.id) return;
		runAudit({
			data: {
				devices: [device.id],
				commands: [
					{
						id: -1,
						command: "RunAudit",
						continue_on_error: false,
					},
				],
			},
		});
	};

	const loading = deviceLoading || auditLoading;

	const checks: { name: string; help: string; value: boolean | null }[] = [
		{
			name: "Disk encrypted",
			help: "A LUKS-encrypted volume was detected on the device.",
			value: audit?.disk_encrypted ?? null,
		},
		{
			name: "Password access disabled",
			help: "SSH password login is disabled and key-based login is enabled.",
			value: audit?.password_access_disabled ?? null,
		},
		{
			name: "Running latest release",
			help: "The device is on its target release.",
			value: (audit as DeviceAudit | undefined)?.running_latest_release ?? null,
		},
	];

	return (
		<div className="flex-1 overflow-y-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
			{/* Header with Back Button */}
			<div className="flex items-center space-x-4">
				<button
					type="button"
					onClick={() => navigate(-1)}
					className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
				>
					<ArrowLeft className="w-4 h-4" />
					<span className="text-sm font-medium">Back to Devices</span>
				</button>
			</div>

			{/* Device Header */}
			{device && <DeviceHeader device={device} serial={serial} />}

			{/* Tabs */}
			<div className="border-b border-gray-200">
				<nav className="-mb-px flex space-x-8">
					<Link
						to={`/devices/${serial}`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Overview
					</Link>
					<Link
						to={`/devices/${serial}/commands`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Commands
					</Link>
					<Link
						to={`/devices/${serial}/services`}
						className="block py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
					>
						Services
					</Link>
					<button className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm">
						Audit
					</button>
				</nav>
			</div>

			{/* Audit Content */}
			<div className="bg-white rounded-lg border border-gray-200 p-6 max-w-2xl">
				<div className="flex items-center justify-between mb-4">
					<div>
						<h3 className="text-lg font-semibold text-gray-900">
							Security Audit
						</h3>
						<p className="text-sm text-gray-500 mt-0.5">
							{audit?.checked_at
								? `Last checked ${new Date(audit.checked_at).toLocaleString()}`
								: "Never checked"}
						</p>
					</div>
					<Button
						variant="secondary"
						loading={isRunningAudit}
						onClick={handleRunAudit}
						icon={<RefreshCw className="w-4 h-4" />}
					>
						Run audit now
					</Button>
				</div>

				{loading ? (
					<div className="py-8 text-gray-500">Loading audit...</div>
				) : (
					<div className="divide-y divide-gray-100">
						{checks.map((check) => (
							<div
								key={check.name}
								className="flex items-center justify-between py-3"
							>
								<div className="min-w-0">
									<div className="text-gray-900 font-medium">{check.name}</div>
									<div className="text-sm text-gray-400">{check.help}</div>
								</div>
								<StatusPill value={check.value} />
							</div>
						))}
					</div>
				)}

				{!loading && !audit?.checked_at && (
					<p className="text-sm text-gray-400 mt-4">
						This device has not reported an audit yet. It will report on its
						next 12-hour cycle, on restart, or when you run one now.
					</p>
				)}
			</div>
		</div>
	);
};

export default AuditPage;
