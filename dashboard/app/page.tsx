'use client';

import { useAuth0 } from '@auth0/auth0-react';
import { useRouter } from 'next/navigation';
import { useEffect } from 'react';
import Logo from "@/app/components/logo";

const LoginPage = () => {
  const { isLoading, error, loginWithPopup } = useAuth0();

  if (error) {
    return <div>Oops... {error.message}</div>;
  }

  return (
    <div className="min-h-screen bg-gray-50 flex items-center justify-center">
      {/* Loading overlay - blocks interaction but keeps background visible */}
      {isLoading && (
        <div className="absolute inset-0 flex items-center justify-center z-50 bg-gray-50 bg-opacity-75">
          <div className="bg-white border border-gray-200 rounded-lg p-6 flex flex-col items-center space-y-4 shadow-sm">
            {/* Spinner */}
            <div className="w-8 h-8 border-4 border-blue-500 border-t-transparent rounded-full animate-spin"></div>
            <div className="text-gray-700 text-sm">Loading...</div>
          </div>
        </div>
      )}

      <div className="w-full max-w-md">
        <div className="bg-white border border-gray-200 rounded-lg p-8 shadow-sm">
          <div className="flex flex-col items-center space-y-6">
            <div className="flex flex-col items-center space-y-3">
              <div className="flex flex-row gap-3 items-center">
                <Logo width={36} color="black" />
                <div className="font-bold text-2xl text-gray-900">Smith</div>
              </div>
              <div className="text-gray-600 text-center">Teton&#39;s Fleet Management System</div>
            </div>

            <button
              disabled={isLoading}
              onClick={() => loginWithPopup()}
              className={`w-full py-3 px-4 rounded-md font-medium transition-colors ${
                isLoading
                  ? 'bg-gray-100 text-gray-400 cursor-not-allowed'
                  : 'bg-green-600 text-white hover:bg-green-700 cursor-pointer'
              }`}
            >
              {isLoading ? 'Logging in...' : 'Log In'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function Home() {
  const { isLoading, isAuthenticated } = useAuth0();
  const router = useRouter();

  useEffect(() => {
    if (!isLoading && isAuthenticated) {
      router.push('/dashboard');
    }
  }, [isLoading, isAuthenticated, router]);

  if (!isLoading && isAuthenticated) {
    return null; // or a loading spinner while redirecting
  }

  return <LoginPage/>;
}