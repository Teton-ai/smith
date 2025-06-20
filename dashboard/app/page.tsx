'use client';

import { useAuth0 } from '@auth0/auth0-react';
import LoginButton from './components/login-button';
import LogoutButton from './components/logout-button';
import Profile from './components/profile';
import AuthDebug from './components/auth-debug';

export default function Home() {
  const { isLoading, error } = useAuth0();

  if (error) {
    return <div>Oops... {error.message}</div>;
  }

  if (isLoading) {
    return <div>Loading...</div>;
  }

  return (
    <main className="container mx-auto px-4 py-8">
      <AuthDebug />
      <div className="flex justify-between items-center mb-8">
        <h1 className="text-3xl font-bold">My Next.js Auth0 App</h1>
        <div className="space-x-4">
          <LoginButton />
          <LogoutButton />
        </div>
      </div>

      <Profile />
    </main>
  );
}
