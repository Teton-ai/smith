import { useAuth0 } from "@auth0/auth0-react";
import axios, { type AxiosRequestConfig } from "axios";
import { useConfig } from "./hooks/config";

export const useClientMutator = <T>() => {
	const { isAuthenticated, getAccessTokenSilently } = useAuth0();
	const { config } = useConfig();

	const fetcher = async (req: AxiosRequestConfig): Promise<T> => {
		if (!isAuthenticated) {
			throw new Error("User not authenticated");
		}

		const token = await getAccessTokenSilently();
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
