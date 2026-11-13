import { Button, Card } from "@teton/smith-ui";
import {
	CheckCircle2,
	Info,
	MinusCircle,
	RefreshCw,
	XCircle,
} from "lucide-react";
import { useParams } from "react-router";
import { useGetDeviceInfo, useIssueCommandsToDevices } from "@/app/api-client";
import { type DeviceAudit, useDeviceAudit } from "../audit/useDeviceAudit";
import { DeviceDetailLayout } from "../DeviceDetailLayout";

/** `info` is for facts that aren't good or bad on their own, like the boot slot. */
type CheckStatus = "pass" | "fail" | "info" | "unknown";

interface Check {
	name: string;
	help: string;
	status: CheckStatus;
	/** text in the pill, defaults to Yes / No / Unknown */
	label?: string;
}

const fromBool = (value: boolean | null | undefined) => ({
	status: (value == null ? "unknown" : value ? "pass" : "fail") as CheckStatus,
});

const slotLabels = { a: "Slot A", b: "Slot B", single: "Single slot" };

const rootfsSlot = (slot: DeviceAudit["rootfs_slot"] | undefined) =>
	slot == null
		? { status: "unknown" as const }
		: { status: "info" as const, label: slotLabels[slot] };

const secureBootStates = {
	on: { status: "pass" as const, label: "On" },
	off: { status: "fail" as const, label: "Off" },
	// anyone can enroll keys in setup mode, so it doesn't protect anything
	setup: { status: "fail" as const, label: "Setup mode" },
};

const secureBoot = (state: DeviceAudit["uefi_secure_boot"] | undefined) =>
	state == null ? { status: "unknown" as const } : secureBootStates[state];

const fallbackSlot = (audit: DeviceAudit | undefined) =>
	// a single-slot unit has nothing to fall back to, which isn't unknown
	audit?.rootfs_slot === "single"
		? { status: "info" as const, label: "No other slot" }
		: fromBool(audit?.rootfs_other_slot_bootable);

const pillStyles: Record<
	CheckStatus,
	{ className: string; icon: typeof CheckCircle2; label: string }
> = {
	pass: { className: "text-green-600", icon: CheckCircle2, label: "Yes" },
	fail: { className: "text-red-600", icon: XCircle, label: "No" },
	info: { className: "text-gray-700", icon: Info, label: "" },
	unknown: { className: "text-gray-400", icon: MinusCircle, label: "Unknown" },
};

/** Renders the status pill for a single audit check. */
const StatusPill = ({ status, label }: Pick<Check, "status" | "label">) => {
	const style = pillStyles[status];
	const Icon = style.icon;
	return (
		<span
			className={`inline-flex items-center gap-1.5 text-sm font-medium whitespace-nowrap ${style.className}`}
		>
			<Icon className="w-4 h-4" />
			{label ?? style.label}
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

	const checks: Check[] = [
		{
			name: "Disk encrypted",
			help: "A LUKS-encrypted volume was detected on the device.",
			...fromBool(audit?.disk_encrypted),
		},
		{
			name: "Password access disabled",
			help: "SSH password login is disabled and key-based login is enabled.",
			...fromBool(audit?.password_access_disabled),
		},
		{
			name: "Console logins closed",
			help: "No login prompt on the screen, serial or USB ports, and console login is refused.",
			...fromBool(audit?.console_logins_closed),
		},
		{
			name: "Reboot watchdog set",
			help: "A hung shutdown resets the device instead of hanging.",
			...fromBool(audit?.reboot_watchdog_set),
		},
		{
			name: "Rootfs slot",
			help: "Which of the two root filesystems the device booted from.",
			...rootfsSlot(audit?.rootfs_slot),
		},
		{
			name: "Fallback slot bootable",
			help: "If an update breaks the running system, the device can still boot its other root filesystem.",
			...fallbackSlot(audit),
		},
		{
			name: "UEFI Secure Boot",
			help: "The firmware only boots a kernel signed with the device's enrolled keys.",
			...secureBoot(audit?.uefi_secure_boot),
		},
		{
			name: "Running latest release",
			help: "The device is on its target release.",
			...fromBool((audit as DeviceAudit | undefined)?.running_latest_release),
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
								<StatusPill status={check.status} label={check.label} />
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
