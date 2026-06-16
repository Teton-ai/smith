import { useMutation } from "@tanstack/react-query";
import { useCallback } from "react";
import { useClientMutator } from "./api-client-mutator";

// Triggering a recipe is a server-side operation: the API loads the recipe's
// commands and gates the call on `recipes:trigger`, so a user who cannot issue
// freeform/tunnel commands directly can still run a vetted recipe that contains
// them. (Mirrors orval's generated mutation hooks; on the next
// `make gen-api-client` this can be replaced by the generated `useTriggerRecipe`.)
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
