import { Button } from "@teton/smith-ui";
import type { ReactNode } from "react";
import {
	useApplySettingsForDevices,
	useGetSettingsForDevice,
	useUpdateSettingsForDevices,
} from "@/app/api-client";
import { Modal } from "@/app/components/modal";
import {
	applyErrorMessage,
	type BulkEditTarget,
	settingsErrorMessage,
} from "@/app/utils/deviceSettings";
import SettingsEditor from "./SettingsEditor";

const plural = (count: number, noun: string) =>
	`${count} ${noun}${count === 1 ? "" : "s"}`;

/** Confirm before queuing ApplySettings, for one device or a selection. */
export const ApplySettingsConfirmModal = ({
	open,
	label,
	intro,
	pending,
	error,
	onConfirm,
	onClose,
}: {
	open: boolean;
	/** "Apply" or "Re-apply". */
	label: string;
	intro?: ReactNode;
	pending: boolean;
	error?: string;
	onConfirm: () => void;
	onClose: () => void;
}) => (
	<Modal
		open={open}
		onClose={onClose}
		title={`${label} settings`}
		footer={
			<>
				<Button variant="soft" tone="gray" disabled={pending} onClick={onClose}>
					Cancel
				</Button>
				<Button variant="solid" loading={pending} onClick={onConfirm}>
					{label}
				</Button>
			</>
		}
	>
		<div className="space-y-2 text-sm text-gray-700">
			{intro && <p>{intro}</p>}
			<p>
				Applying driver settings can reload the Wi-Fi driver: a device goes
				offline for about 10 to 20 s.
			</p>
			<p>
				If smithd cannot reach the API within 90 s afterwards, it reverts the
				change by itself.
			</p>
			{error && <p className="text-red-600">{error}</p>}
		</div>
	</Modal>
);

const BulkEditBody = ({
	referenceId,
	deviceIds,
	onSaved,
	onClose,
}: {
	referenceId: number;
	deviceIds: number[];
	onSaved: (message: string) => void;
	onClose: () => void;
}) => {
	const { data: settings, isLoading } = useGetSettingsForDevice(
		String(referenceId),
	);
	const { mutate, isPending, error } = useUpdateSettingsForDevices({
		mutation: {
			onSuccess: (versions) =>
				onSaved(
					`Settings saved for ${plural(versions.length, "device")}. Apply them to send them.`,
				),
		},
	});
	if (isLoading || !settings) {
		return <div className="h-4 animate-pulse rounded bg-gray-100" />;
	}
	return (
		<SettingsEditor
			bulk
			resolved={settings.resolved}
			defaults={settings.defaults}
			unsupported={settings.unsupported}
			saving={isPending}
			error={error ? settingsErrorMessage(error) : undefined}
			onSave={(changes) => mutate({ data: { devices: deviceIds, changes } })}
			onCancel={onClose}
		/>
	);
};

export const BulkEditSettingsModal = ({
	open,
	onClose,
	onSaved,
	deviceIds,
	target,
}: {
	open: boolean;
	onClose: () => void;
	/** Called with a confirmation once the pins are stored. */
	onSaved: (message: string) => void;
	deviceIds: number[];
	target: BulkEditTarget;
}) => (
	<Modal
		open={open}
		onClose={onClose}
		title={`Edit settings of ${plural(deviceIds.length, "device")}`}
		footer={
			target.kind === "blocked" ? (
				<Button variant="soft" tone="gray" onClick={onClose}>
					Close
				</Button>
			) : undefined
		}
	>
		{target.kind === "blocked" ? (
			<p className="text-sm text-gray-700">{target.message}</p>
		) : (
			// Modal renders nothing while closed, so the settings GET runs only when open.
			<BulkEditBody
				referenceId={target.referenceId}
				deviceIds={deviceIds}
				onSaved={onSaved}
				onClose={onClose}
			/>
		)}
	</Modal>
);

export const BulkApplySettingsModal = ({
	open,
	onClose,
	onQueued,
	deviceIds,
}: {
	open: boolean;
	onClose: () => void;
	/** Called with the queued and skipped counts once the apply is accepted. */
	onQueued: (message: string) => void;
	deviceIds: number[];
}) => {
	const { mutate, isPending, error, reset } = useApplySettingsForDevices({
		mutation: {
			onSuccess: (receipt) => {
				const skipped = deviceIds.length - receipt.commands.length;
				onQueued(
					`Apply queued for ${plural(receipt.commands.length, "device")}${skipped > 0 ? `, ${skipped} skipped (never given settings)` : ""}.`,
				);
			},
		},
	});
	const close = () => {
		reset();
		onClose();
	};
	return (
		<ApplySettingsConfirmModal
			open={open}
			label="Apply"
			intro={`Apply the managed settings to ${plural(deviceIds.length, "device")}. Devices that were never given settings are skipped.`}
			pending={isPending}
			error={error ? applyErrorMessage(error) : undefined}
			onConfirm={() => mutate({ data: { devices: deviceIds } })}
			onClose={close}
		/>
	);
};
