import { useQueryClient } from "@tanstack/react-query";
import { Badge, Button, Card } from "@teton/smith-ui";
import { Pin, PinOff, Radio } from "lucide-react";
import { useState } from "react";
import {
	getGetDeviceReleaseIntentQueryKey,
	useGetDeviceReleaseIntent,
	useSetDeviceReleaseIntent,
} from "@/app/api-client";

/** A device is either held at one release or following its distribution's
 *  latest. The third state — neither — only exists for devices the backfill
 *  has not reached, so it is shown as something to resolve rather than a
 *  mode you can pick. */
export const ReleaseIntentCard = ({
	deviceId,
	currentReleaseId,
}: {
	deviceId: number;
	currentReleaseId?: number;
}) => {
	const queryClient = useQueryClient();
	const { data: intent } = useGetDeviceReleaseIntent(deviceId);
	const [error, setError] = useState<string | null>(null);

	const { mutate: setIntent, isPending } = useSetDeviceReleaseIntent({
		mutation: {
			onSuccess: () => {
				setError(null);
				queryClient.invalidateQueries({
					queryKey: getGetDeviceReleaseIntentQueryKey(deviceId),
				});
			},
			onError: () => setError("Could not change this device's release intent."),
		},
	});

	if (!intent) return null;

	const follow = () => setIntent({ deviceId, data: { follows_latest: true } });

	const hold = () => {
		// Holding means "stay on what you are running now"; without a release to
		// pin to there is nothing to hold, so the control stays disabled.
		if (!currentReleaseId) return;
		setIntent({
			deviceId,
			data: { follows_latest: false, pinned_release_id: currentReleaseId },
		});
	};

	return (
		<Card className="p-5">
			<div className="flex items-start justify-between gap-4">
				<div>
					<div className="flex items-center gap-2">
						<h3 className="text-sm font-semibold text-gray-900">
							Release intent
						</h3>
						{intent.state === "pinned" && <Badge variant="orange">Held</Badge>}
						{intent.state === "following" && (
							<Badge variant="green">Following latest</Badge>
						)}
						{intent.state === "unmanaged" && (
							<Badge variant="gray">Not set</Badge>
						)}
					</div>
					<p className="mt-1 text-sm text-gray-500">
						{intent.state === "pinned" &&
							"This device stays on its pinned release. Distribution rollouts pass it by."}
						{intent.state === "following" &&
							"This device picks up the distribution's latest release on its next check-in."}
						{intent.state === "unmanaged" &&
							"No intent recorded. It keeps whatever target it was last given and is skipped by rollouts."}
					</p>
					{error && <p className="mt-2 text-sm text-red-600">{error}</p>}
				</div>
				<div className="flex shrink-0 gap-2">
					<Button
						tone="green"
						variant={intent.state === "following" ? "solid" : "soft"}
						size="sm"
						onClick={follow}
						disabled={isPending || intent.state === "following"}
					>
						<Radio className="mr-1.5 h-4 w-4" />
						Follow latest
					</Button>
					<Button
						tone="orange"
						variant={intent.state === "pinned" ? "solid" : "soft"}
						size="sm"
						onClick={hold}
						disabled={
							isPending || intent.state === "pinned" || !currentReleaseId
						}
					>
						{intent.state === "pinned" ? (
							<Pin className="mr-1.5 h-4 w-4" />
						) : (
							<PinOff className="mr-1.5 h-4 w-4" />
						)}
						Hold here
					</Button>
				</div>
			</div>
		</Card>
	);
};
