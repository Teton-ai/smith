// Hand-written data hooks for the users/roles endpoints, mirroring the shape of
// the Orval-generated client (app/api-client.ts). They use the same auth-aware
// mutator and React Query so they behave identically. Once the backend is up,
// `npm run gen-api-client` will emit equivalent `useGetUsers`/`useGetRoles`
// hooks into api-client.ts and this file can be removed in favor of those.
import { type UseQueryResult, useQuery } from "@tanstack/react-query";
import { useClientMutator } from "@/app/api-client-mutator";

export interface PermissionInfo {
	action: string;
	resource: string;
}

export interface UserWithRoles {
	id: number;
	email: string | null;
	roles: string[];
	created_at: string;
	updated_at: string;
}

export interface RoleInfo {
	name: string;
	description: string;
	inherits: string[];
	permissions: PermissionInfo[];
	effective_permissions: PermissionInfo[];
}

export const getGetUsersQueryKey = () => ["/users"] as const;
export const getGetRolesQueryKey = () => ["/roles"] as const;

export function useGetUsers(): UseQueryResult<UserWithRoles[], unknown> {
	const fetcher = useClientMutator<UserWithRoles[]>();
	return useQuery({
		queryKey: getGetUsersQueryKey(),
		queryFn: ({ signal }) => fetcher({ url: "/users", method: "GET", signal }),
	});
}

export function useGetRoles(): UseQueryResult<RoleInfo[], unknown> {
	const fetcher = useClientMutator<RoleInfo[]>();
	return useQuery({
		queryKey: getGetRolesQueryKey(),
		queryFn: ({ signal }) => fetcher({ url: "/roles", method: "GET", signal }),
	});
}
