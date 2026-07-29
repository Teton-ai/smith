import { Badge, Button, Card } from "@teton/smith-ui";
import { Eye, EyeOff, KeyRound } from "lucide-react";
import { useState } from "react";
import { useParams } from "react-router";
import { useGetDeviceInfo, useGetVariablesForDevice } from "@/app/api-client";
import { DeviceDetailLayout } from "../DeviceDetailLayout";

/** Length-agnostic mask so a hidden secret never leaks its length. */
const MASK = "••••••••••••";

/** Device variables tab. Values are secrets, so they are masked by default and
 *  only shown after the user reveals them. The count and reveal toggle live in
 *  the tab bar — the tab label already names the section, so the list needs no
 *  header of its own. */
const VariablesPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const [revealed, setRevealed] = useState(false);

	const { data: device } = useGetDeviceInfo(serial);
	const deviceId = device?.id;
	const { data: variables, isLoading } = useGetVariablesForDevice(
		deviceId ?? 0,
		{ query: { enabled: !!deviceId } },
	);

	const hasVariables = variables && variables.length > 0;

	return (
		<DeviceDetailLayout
			serial={serial}
			device={device}
			activeTab="variables"
			tabActions={
				hasVariables ? (
					<>
						<Button
							variant="soft"
							tone="gray"
							size="sm"
							onClick={() => setRevealed((r) => !r)}
							icon={
								revealed ? (
									<EyeOff className="w-4 h-4" />
								) : (
									<Eye className="w-4 h-4" />
								)
							}
						>
							{revealed ? "Hide secrets" : "Reveal secrets"}
						</Button>
						<Badge variant="yellow" pill>
							{variables.length}
						</Badge>
					</>
				) : undefined
			}
		>
			<Card className="overflow-hidden">
				{isLoading ? (
					<div className="px-4 py-6 text-gray-500">Loading variables...</div>
				) : hasVariables ? (
					<div className="divide-y divide-gray-100">
						{variables.map((variable) => (
							<div
								key={variable.id}
								className="flex items-center justify-between gap-4 px-4 py-3"
							>
								<span className="font-mono text-sm font-medium text-gray-900 break-all">
									{variable.name}
								</span>
								<span className="font-mono text-sm text-gray-900 text-right break-all min-w-0">
									{revealed ? variable.value : MASK}
								</span>
							</div>
						))}
					</div>
				) : (
					<div className="text-center py-12">
						<KeyRound className="w-12 h-12 text-gray-300 mx-auto mb-4" />
						<p className="text-gray-500">
							No variables are set on this device.
						</p>
					</div>
				)}
			</Card>
		</DeviceDetailLayout>
	);
};

export default VariablesPage;
