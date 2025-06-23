'use client';

import { Auth0Provider } from '@auth0/auth0-react';
import { ReactNode } from 'react';

interface Auth0ProviderWrapperProps {
  children: ReactNode;
}

export default function Auth0ProviderWrapper({ children }: Auth0ProviderWrapperProps) {
  return (
    <Auth0Provider
      domain="https://teton-ai.eu.auth0.com"
      clientId="1XPAp9LsuOddURHVDNwiL2H8rhCPhTGE"
      authorizationParams={{
        redirect_uri: "http://localhost:3000",
        audience: "https://teton.ai",
        scope: "openid profile email"
      }}
    >
      {children}
    </Auth0Provider>
  );
}
