import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";
import {
	type Device,
	getGetFavoriteDevicesQueryKey,
	useAddFavoriteDevice,
	useGetFavoriteDevices,
	useRemoveFavoriteDevice,
} from "@/app/api-client";

const favoritesQueryKey = getGetFavoriteDevicesQueryKey();

// Single shared favorites cache entry: the devices list hearts, the dashboard
// card, and the favorites page all read/write the same query key, so a toggle
// anywhere is reflected everywhere without extra polling.
export const useFavorites = () => {
	const queryClient = useQueryClient();

	const { data: favorites = [], isLoading } = useGetFavoriteDevices();

	const addMutation = useAddFavoriteDevice();
	const removeMutation = useRemoveFavoriteDevice();

	const favoriteIds = useMemo(
		() => new Set(favorites.map((device) => device.id)),
		[favorites],
	);

	const isFavorite = useCallback(
		(deviceId: number) => favoriteIds.has(deviceId),
		[favoriteIds],
	);

	// Optimistic flip; snapshot restored on error, server state refetched on
	// settle either way. Resolves to false on failure so callers can surface it.
	const toggle = useCallback(
		async (device: Device): Promise<boolean> => {
			await queryClient.cancelQueries({ queryKey: favoritesQueryKey });

			const previous = queryClient.getQueryData<Device[]>(favoritesQueryKey);
			const currentlyFavorite = (previous ?? []).some(
				(d) => d.id === device.id,
			);

			queryClient.setQueryData<Device[]>(favoritesQueryKey, (old = []) =>
				currentlyFavorite
					? old.filter((d) => d.id !== device.id)
					: [...old, device],
			);

			const mutation = currentlyFavorite ? removeMutation : addMutation;
			try {
				await mutation.mutateAsync({ deviceId: device.id });
				return true;
			} catch {
				queryClient.setQueryData(favoritesQueryKey, previous);
				return false;
			} finally {
				queryClient.invalidateQueries({ queryKey: favoritesQueryKey });
			}
		},
		[queryClient, addMutation, removeMutation],
	);

	// Mutation instances are local to each hook consumer, so this flag only
	// covers toggles started by that consumer (one button disables itself,
	// not every heart on the page).
	const isToggling = addMutation.isPending || removeMutation.isPending;

	return { favorites, isFavorite, toggle, isToggling, isLoading };
};
