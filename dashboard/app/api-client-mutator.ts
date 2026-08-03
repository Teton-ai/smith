import { useAuth0 } from "@auth0/auth0-react";
import axios, { type AxiosRequestConfig } from "axios";
import { useConfig } from "./hooks/config";

// Guards against many concurrent in-flight requests all triggering logout
// when the refresh token expires.
let isLoggingOut = false;

const useRawFetcher = () => {
	const { isAuthenticated, getAccessTokenSilently, logout } = useAuth0();
	const { config } = useConfig();

	return async (req: AxiosRequestConfig) => {
		if (!isAuthenticated) {
			throw new Error("User not authenticated");
		}

		let token: string;
		try {
			token = await getAccessTokenSilently();
		} catch (err) {
			const unrecoverableAuthErrors = [
				"login_required",
				"consent_required",
				"access_denied",
				"invalid_grant",
			];
			const errorCode = (err as { error?: string } | null)?.error ?? "";
			if (unrecoverableAuthErrors.includes(errorCode) && !isLoggingOut) {
				isLoggingOut = true;
				logout({
					logoutParams: {
						returnTo:
							typeof window !== "undefined" ? window.location.origin : "",
					},
				});
			}
			throw err;
		}

		return axios({
			...req,
			paramsSerializer: {
				indexes: null,
			},
			baseURL: config?.API_BASE_URL || "http://127.0.0.1:8080",
			headers: {
				...req?.headers,
				Authorization: `Bearer ${token}`,
			},
		});
	};
};

export const useClientMutator = <T>() => {
	const rawFetcher = useRawFetcher();
	return async (req: AxiosRequestConfig): Promise<T> => {
		const res = await rawFetcher(req);
		return res.data as T;
	};
};

/**
 * Like `useClientMutator`, but also exposes the response status. Needed when a
 * caller must tell "the server created this" (201) apart from "the server
 * matched an existing row" (200) - e.g. idempotent POSTs where only the former
 * is safe to compensate for on a later failure.
 */
export const useClientMutatorWithStatus = <T>() => {
	const rawFetcher = useRawFetcher();
	return async (
		req: AxiosRequestConfig,
	): Promise<{ data: T; status: number }> => {
		const res = await rawFetcher(req);
		return { data: res.data as T, status: res.status };
	};
};
