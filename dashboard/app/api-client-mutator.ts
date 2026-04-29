import { useAuth0 } from "@auth0/auth0-react";
import axios, { type AxiosRequestConfig } from "axios";
import { useConfig } from "./hooks/config";

// Guards against many concurrent in-flight requests all triggering logout
// when the refresh token expires.
let isLoggingOut = false;

export const useClientMutator = <T>() => {
	const { isAuthenticated, getAccessTokenSilently, logout } = useAuth0();
	const { config } = useConfig();

	const fetcher = async (req: AxiosRequestConfig): Promise<T> => {
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
			const errorCode =
				(err as { error?: string } | null)?.error ?? "";
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

		const res = await axios({
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
		return res.data as T;
	};

	return fetcher;
};
