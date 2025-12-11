// hooks/useConfig.ts
import { useEffect, useState } from 'react';

interface Config {
  API_BASE_URL: string;
  AUTH0_DOMAIN: string;
  AUTH0_CLIENT_ID: string;
  AUTH0_REDIRECT_URI: string;
  AUTH0_AUDIENCE: string;
  DASHBOARD_EXCLUDED_LABELS?: string;
}

interface ConfigResponse {
  env: Config;
}

let configCache: Config | null = null;
let configPromise: Promise<Config> | null = null;

const fetchConfig = async (): Promise<Config> => {
  if (configCache) return configCache;

  if (!configPromise) {
    configPromise = fetch('/api/config')
      .then(res => res.json())
      .then((data: ConfigResponse) => {
        configCache = data.env;
        return data.env;
      })
      .catch(error => {
        configPromise = null; // Reset promise on error to allow retry
        throw error;
      });
  }

  return configPromise;
};

export const useConfig = () => {
  const [config, setConfig] = useState<Config | null>(configCache);
  const [loading, setLoading] = useState(!configCache);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (configCache) return;

    fetchConfig()
      .then(config => {
        setConfig(config);
        setLoading(false);
      })
      .catch(err => {
        setError(err);
        setLoading(false);
      });
  }, []);

  return { config, loading, error };
};
