import { useMutation } from "@tanstack/react-query";
import { useCallback } from "react";
import { useClientMutator } from "./api-client-mutator";

// STOPGAP: this should be the orval-generated `useTriggerRecipe` from
// api-client.ts. The generated client is currently stale (it predates several
// merged PRs and a full `make gen-api-client` produces a ~4k-line diff with
// type errors), so the hook isn't there yet. Replace this with the generated
// hook once the client is regenerated.
export const useTriggerRecipe = () => {
	const triggerRecipe = useClientMutator<void>();

	const mutationFn = useCallback(
		({ recipeId, devices }: { recipeId: number; devices: number[] }) =>
			triggerRecipe({
				url: `/commands/recipes/${recipeId}/trigger`,
				method: "POST",
				headers: { "Content-Type": "application/json" },
				data: { devices },
			}),
		[triggerRecipe],
	);

	return useMutation({ mutationFn });
};
