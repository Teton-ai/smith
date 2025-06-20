'use client';

import { useAuth0 } from '@auth0/auth0-react';
import { useEffect } from 'react';

export default function AuthDebug() {
  const { error, isLoading, isAuthenticated, user } = useAuth0();

  useEffect(() => {
    if (error) {
      console.error('Auth0 Error:', error);
    }
  }, [error]);

  if (isLoading) return <div>Loading...</div>;

  if (error) {
    return (
      <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded">
        <strong>Auth Error:</strong> {error.message}
        <details className="mt-2">
          <summary>Error Details</summary>
          <pre className="text-xs mt-2 overflow-auto">
            {JSON.stringify(error, null, 2)}
          </pre>
        </details>
      </div>
    );
  }

  return null;
}
