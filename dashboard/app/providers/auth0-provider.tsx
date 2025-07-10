'use client';

import { Auth0Provider } from '@auth0/auth0-react';
import { ReactNode } from 'react';
import { useConfig } from '@/app/hooks/config';

interface Auth0ProviderWrapperProps {
  children: ReactNode;
}

export default function Auth0ProviderWrapper({ children }: Auth0ProviderWrapperProps) {
  const { config, loading } = useConfig();
  if (loading) {
    return null;
  }
  return (
    <Auth0Provider
      domain={config!.AUTH0_DOMAIN}
      clientId={config!.AUTH0_CLIENT_ID}
      authorizationParams={{
        redirect_uri: config!.AUTH0_REDIRECT_URI,
        audience: config!.AUTH0_AUDIENCE,
        scope: "openid profile email"
      }}
    >
      {children}
    </Auth0Provider>
  );
}
