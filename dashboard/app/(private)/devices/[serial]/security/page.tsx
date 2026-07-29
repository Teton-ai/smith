import { Button, Card } from "@teton/smith-ui";
import { CheckCircle2, MinusCircle, RefreshCw, XCircle } from "lucide-react";
import { useParams } from "react-router";
import { useGetDeviceInfo, useIssueCommandsToDevices } from "@/app/api-client";
import { type DeviceAudit, useDeviceAudit } from "../audit/useDeviceAudit";
import { DeviceDetailLayout } from "../DeviceDetailLayout";

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

/** Security audit tab. Fetches the audit, can trigger a fresh run, and reflects
 *  the result. The last-checked stamp and the run button live in the tab bar —
 *  the tab label already names the section, so the list needs no header. */
const SecurityPage = () => {
	const params = useParams();
	const serial = params.serial as string;

	const { data: device } = useGetDeviceInfo(serial);
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
		if (!device?.id) return;
		runAudit({
			data: {
				devices: [device.id],
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
		<DeviceDetailLayout
			serial={serial}
			device={device}
			activeTab="security"
			tabActions={
				<>
					<span className="text-sm text-gray-500">
						{audit?.checked_at
							? `Last checked ${new Date(audit.checked_at).toLocaleString()}`
							: "Never checked"}
					</span>
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
				</>
			}
		>
			<Card className="overflow-hidden">
				{isLoading ? (
					<div className="px-4 py-6 text-gray-500">Loading audit...</div>
				) : (
					<div className="divide-y divide-gray-100">
						{checks.map((check) => (
							<div
								key={check.name}
								className="flex items-center justify-between gap-4 px-4 py-3"
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
			</Card>

			{!isLoading && !audit?.checked_at && (
				<p className="text-sm text-gray-400">
					This device has not reported an audit yet. It will report on its next
					12-hour cycle, on restart, or when you run one now.
				</p>
			)}
		</DeviceDetailLayout>
	);
};

export default SecurityPage;
