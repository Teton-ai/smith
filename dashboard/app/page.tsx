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
    <div className="fixed top-0 left-0 z-50 flex items-center w-screen h-screen bg-[#1E1E1E]">
      {/* Loading overlay - blocks interaction but keeps background visible */}
      {isLoading && (
        <div className="absolute inset-0 flex items-center justify-center z-50 pointer-events-none">
          <div className="bg-black bg-opacity-50 backdrop-blur-sm rounded-lg p-6 flex flex-col items-center space-y-4 pointer-events-auto">
            {/* Spinner */}
            <div className="w-8 h-8 border-4 border-[#66C995] border-t-transparent rounded-full animate-spin"></div>
            <div className="text-white text-sm">Loading...</div>
          </div>
        </div>
      )}

      <div className="flex flex-row w-full h-full">
        <div className="flex flex-col flex-1 gap-10 items-center justify-center bg-[#303035]">
          <div className="flex flex-col items-center space-y-2">
            <div className="flex flex-row gap-3 items-center">
              <Logo width={36} />
              <div className="font-bold text-2xl">Smith</div>
            </div>
            <div>Teton&#39;s Fleet Management System</div>
          </div>

          <button
            disabled={isLoading}
            onClick={() => loginWithPopup()}
            className={`w-full max-w-md py-3 rounded-xl font-medium transition-all duration-200 cursor-pointer ${
              isLoading
                ? 'bg-gray-400 cursor-not-allowed opacity-50'
                : 'bg-[#66C995] hover:opacity-90'
            }`}
            style={{
              color: '#F2F4F4'
            }}
          >
            {isLoading ? 'Logging in...' : 'Log In'}
          </button>
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