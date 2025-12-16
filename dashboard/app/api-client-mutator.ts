import { useConfig } from "./hooks/config";
import { useAuth0 } from "@auth0/auth0-react";

export const useClientMutator = <T>() => {
  const { isAuthenticated, getAccessTokenSilently } = useAuth0();
  const { config } = useConfig();

  const fetcher = async (input: RequestInfo | URL, init?: RequestInit): Promise<T> => {
    if (!isAuthenticated) {
      throw new Error('User not authenticated');
    }

    const token = await getAccessTokenSilently();
    const BASE_URL = config?.API_BASE_URL || 'http://127.0.0.1:8080';
    let theInput: RequestInfo | URL;
    if (typeof input === "string") {
      theInput = `${BASE_URL}${input}`
    } else if ("url" in input) {
      theInput = {
        ...input,
        url: `${BASE_URL}${input.url}` 
      }
    } else {
      theInput = input;
    }
    const res = await fetch(theInput,
      {
        ...init,
        headers: {
          ...init?.headers,
          'Authorization': `Bearer ${token}`,
          'Content-Type': 'application/json',
       }
     })
    return await res.json();
  }

  return fetcher;
}
