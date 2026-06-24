import { Eye, EyeOff, KeyRound } from "lucide-react";
import { useState } from "react";
import { useGetVariablesForDevice } from "@/app/api-client";
import { Button, Panel, SECTION_THEMES } from "@/app/components/ui";

/** Length-agnostic mask so a hidden secret never leaks its length. */
const MASK = "••••••••••••";

/** Device variables panel for the overview. Values are secrets, so they are
 *  masked by default and only shown after the user reveals them. */
const DeviceVariables = ({ deviceId }: { deviceId?: number }) => {
	const [revealed, setRevealed] = useState(false);
	const { data: variables, isLoading } = useGetVariablesForDevice(
		deviceId ?? 0,
		{ query: { enabled: !!deviceId } },
	);

	const hasVariables = variables && variables.length > 0;

	return (
		<Panel
			title="Variables"
			icon={KeyRound}
			theme={SECTION_THEMES.yellow}
			count={variables?.length}
			actions={
				hasVariables ? (
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
				) : undefined
			}
		>
			{isLoading ? (
				<div className="py-6 text-gray-500">Loading variables...</div>
			) : hasVariables ? (
				<div className="divide-y divide-gray-100">
					{variables.map((variable) => (
						<div
							key={variable.id}
							className="flex items-center justify-between gap-4 py-3"
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
				<p className="text-sm text-gray-500">
					No variables are set on this device.
				</p>
			)}
		</Panel>
	);
};

export default DeviceVariables;
