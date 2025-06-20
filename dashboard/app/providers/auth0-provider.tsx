'use client';

import { Auth0Provider } from '@auth0/auth0-react';
import { ReactNode } from 'react';

interface Auth0ProviderWrapperProps {
  children: ReactNode;
}

export default function Auth0ProviderWrapper({ children }: Auth0ProviderWrapperProps) {
  return (
    <Auth0Provider
      domain="https://tenant-ai-dev.eu.auth0.com"
      clientId="JxiK5L2zMPSLD4arcUJvQRQ5pNCP6mc5"
      authorizationParams={{
        redirect_uri: "http://localhost:3000",
        audience: "https://teton.dev",
        scope: "openid profile email"
      }}
    >
      {children}
    </Auth0Provider>
  );
}
