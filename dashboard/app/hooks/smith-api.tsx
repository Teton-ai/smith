import { useState, useCallback } from 'react';
import { useAuth0 } from '@auth0/auth0-react';
import axios, { AxiosRequestConfig, AxiosError } from 'axios';

type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';

interface SmithAPIHook {
  callAPI: <T = unknown>(method: HttpMethod, path: string, body?: unknown) => Promise<T | null>;
  loading: boolean;
  error: string | null;
}

const useSmithAPI = (): SmithAPIHook => {
  const { isAuthenticated, getAccessTokenSilently } = useAuth0();
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  const BASE_URL = 'http://127.0.0.1:8080';

  const callAPI = useCallback(async <T = unknown>(
    method: HttpMethod = 'GET',
    path: string = '/',
    body: unknown = null
  ): Promise<T | null> => {
    if (!isAuthenticated) {
      setError('User not authenticated');
      return null;
    }

    setLoading(true);
    setError(null);

    try {
      const token = await getAccessTokenSilently();

      const config: AxiosRequestConfig = {
        method,
        url: `${BASE_URL}${path}`,
        headers: {
          'Authorization': `Bearer ${token}`,
          'Content-Type': 'application/json',
        },
      };

      if (body && ['POST', 'PUT', 'PATCH'].includes(method.toUpperCase())) {
        config.data = body;
      }

      const response = await axios(config);
      return response.data as T;

    } catch (err: unknown) {
      const axiosError = err as AxiosError<{ message?: string }>;
      const errorMessage = axiosError.response?.data?.message || axiosError.message || 'An error occurred';
      setError(errorMessage);
      return null;
    } finally {
      setLoading(false);
    }
  }, [isAuthenticated, getAccessTokenSilently]);

  return { callAPI, loading, error };
};

export default useSmithAPI;
