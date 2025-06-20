'use client';

import { useAuth0 } from '@auth0/auth0-react';

export default function LoginButton() {
  const { loginWithPopup, isAuthenticated } = useAuth0();

  if (isAuthenticated) {
    return null;
  }

  return (
    <button
      onClick={() => loginWithPopup()}
      className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
    >
      Log In
    </button>
  );
}
