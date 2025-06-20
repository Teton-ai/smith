'use client';

import { useAuth0 } from '@auth0/auth0-react';
import Image from 'next/image';
import { useEffect } from 'react';

export default function Profile() {
  const { user, isAuthenticated, isLoading, getAccessTokenSilently } = useAuth0();

  useEffect(() => {
    const fetchDashboardData = async () => {
      if (isAuthenticated) {
        try {
          // Get the access token
          const token = await getAccessTokenSilently();

          // Call the API endpoint
          const response = await fetch('http://127.0.0.1:8080/dashboard', {
            method: 'GET',
            headers: {
              'Authorization': `Bearer ${token}`,
              'Content-Type': 'application/json',
            },
          });

          if (response.ok) {
            const data = await response.json();
            console.log('Dashboard data:', data);
          } else {
            console.error('API call failed:', response.status, response.statusText);
          }
        } catch (error) {
          console.error('Error fetching dashboard data:', error);
        }
      }
    };

    fetchDashboardData();
  }, [isAuthenticated, getAccessTokenSilently]);

  if (isLoading) {
    return <div>Loading...</div>;
  }

  if (!isAuthenticated || !user) {
    return null;
  }

  return (
    <div className="bg-white shadow rounded-lg p-6">
      <div className="flex items-center space-x-4">
        {user.picture && (
          <Image
            src={user.picture}
            alt={user.name || 'User'}
            width={64}
            height={64}
            className="rounded-full"
          />
        )}
        <div>
          <h2 className="text-xl font-bold">{user.name}</h2>
          <p className="text-gray-600">{user.email}</p>
        </div>
      </div>
    </div>
  );
}
