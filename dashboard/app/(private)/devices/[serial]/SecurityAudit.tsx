import { Button, Panel, SECTION_THEMES } from "@teton/smith-ui";
import {
	CheckCircle2,
	MinusCircle,
	RefreshCw,
	ShieldCheck,
	XCircle,
} from "lucide-react";
import { useIssueCommandsToDevices } from "@/app/api-client";
import { type DeviceAudit, useDeviceAudit } from "./audit/useDeviceAudit";

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

/** Security audit panel for the device overview. Self-contained: fetches the
 *  audit, can trigger a fresh run, and reflects the result. */
const SecurityAudit = ({
	serial,
	deviceId,
}: {
	serial: string;
	deviceId?: number;
}) => {
	const { data: audit, isLoading, refetch } = useDeviceAudit(serial);

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
		if (!deviceId) return;
		runAudit({
			data: {
				devices: [deviceId],
				commands: [{ id: -1, command: "RunAudit", continue_on_error: false }],
			},
		});
	};

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
		<Panel
			title="Security Audit"
			icon={ShieldCheck}
			theme={SECTION_THEMES.rose}
			actions={
				<Button
					variant="soft"
					tone="gray"
					size="sm"
					loading={isRunningAudit}
					onClick={handleRunAudit}
					icon={<RefreshCw className="w-4 h-4" />}
				>
					Run audit now
				</Button>
			}
		>
			<p className="text-sm text-gray-500 mb-3">
				{audit?.checked_at
					? `Last checked ${new Date(audit.checked_at).toLocaleString()}`
					: "Never checked"}
			</p>

			{isLoading ? (
				<div className="py-6 text-gray-500">Loading audit...</div>
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

			{!isLoading && !audit?.checked_at && (
				<p className="text-sm text-gray-400 mt-4">
					This device has not reported an audit yet. It will report on its next
					12-hour cycle, on restart, or when you run one now.
				</p>
			)}
		</Panel>
	);
};

export default SecurityAudit;
