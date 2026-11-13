import { Button } from "@teton/smith-ui";
import type { ReactNode } from "react";
import { Modal } from "@/app/components/modal";

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
