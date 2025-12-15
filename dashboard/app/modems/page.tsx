'use client';

import React, { useState, useEffect } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import {
  Smartphone,
  Search,
  Signal,
  Check,
  X,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";
import { Modem } from '@/models';


const ModemSkeleton = () => (
  <div className="px-4 py-3 animate-pulse">
    <div className="grid grid-cols-4 gap-4 items-center">
      <div className="col-span-2">
        <div className="flex items-center space-x-3">
          <div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0"></div>
          <div className="space-y-1">
            <div className="h-4 bg-gray-300 rounded w-36"></div>
            <div className="h-3 bg-gray-200 rounded w-24"></div>
          </div>
        </div>
      </div>
      <div className="col-span-1">
        <div className="flex items-center space-x-2">
          <div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0"></div>
          <div className="h-4 bg-gray-300 rounded w-28"></div>
        </div>
      </div>
      <div className="col-span-1">
        <div className="flex items-center justify-between">
          <div className="h-3 bg-gray-300 rounded w-12"></div>
          <div className="w-4 h-4 bg-gray-300 rounded"></div>
        </div>
      </div>
    </div>
  </div>
);

const LoadingSkeleton = () => (
  <div className="divide-y divide-gray-200">
    {Array.from({ length: 8 }, (_, i) => (
      <ModemSkeleton key={i} />
    ))}
  </div>
);

const ModemsPage = () => {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { callAPI } = useSmithAPI();
  const [searchTerm, setSearchTerm] = useState('');
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  const { data: modems = [], isLoading: initialLoading } = useQuery({
    queryKey: ['modems'],
    queryFn: () => callAPI<Modem[]>('GET', '/modems'),
    refetchInterval: 5000,
    select: (data) => {
      if (!data) return [];
      return [...data].sort((a, b) =>
        new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime()
      );
    },
  });

  // Initialize search term from URL params
  useEffect(() => {
    const urlSearch = searchParams.get('search') || '';
    setSearchTerm(urlSearch);
  }, [searchParams]);

  const filteredModems = searchTerm === ''
    ? modems
    : modems.filter(modem =>
        modem.imei.toLowerCase().includes(searchTerm.toLowerCase()) ||
        modem.network_provider.toLowerCase().includes(searchTerm.toLowerCase())
      );

  // Auto-hide toast after 3 seconds
  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [toast]);

  const formatTimeAgo = (date: string) => {
    const now = new Date();
    const past = new Date(date);
    const diff = now.getTime() - past.getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d`;
    if (hours > 0) return `${hours}h`;
    return `${minutes}m`;
  };

  const updateSearchInUrl = (newSearchTerm: string) => {
    const params = new URLSearchParams(searchParams);
    if (newSearchTerm) {
      params.set('search', newSearchTerm);
    } else {
      params.delete('search');
    }
    router.push(`/modems?${params.toString()}`, { scroll: false });
  };

  const handleSearchChange = (value: string) => {
    setSearchTerm(value);
    updateSearchInUrl(value);
  };

  return (
    <PrivateLayout id="modems">
      <div className="space-y-6">
        {/* Toast Notification */}
        {toast && (
          <div className={`fixed top-4 right-4 z-50 p-4 rounded-lg shadow-lg border ${
            toast.type === 'success'
              ? 'bg-green-50 text-green-800 border-green-200'
              : 'bg-red-50 text-red-800 border-red-200'
          } transition-all duration-300 ease-in-out`}>
            <div className="flex items-center space-x-2">
              {toast.type === 'success' ? (
                <Check className="w-5 h-5 text-green-600" />
              ) : (
                <X className="w-5 h-5 text-red-600" />
              )}
              <span className="text-sm font-medium">{toast.message}</span>
              <button
                onClick={() => setToast(null)}
                className="ml-2 text-gray-400 hover:text-gray-600"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
          </div>
        )}
        {/* Search and Modem Count */}
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center space-x-4">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
              <input
                type="text"
                placeholder="Search modems..."
                value={searchTerm}
                onChange={(e) => handleSearchChange(e.target.value)}
                className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
              />
            </div>
          </div>

          <div className="mt-4 sm:mt-0 flex items-center space-x-3">
            <span className="text-sm text-gray-500">
              {initialLoading ? 'Loading...' : `${filteredModems.length} modem${filteredModems.length !== 1 ? 's' : ''} shown`}
            </span>
          </div>
        </div>

        {/* Modem List */}
        <div className="border border-gray-200 rounded-lg overflow-hidden bg-white">
          <div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
            <div className="grid grid-cols-4 gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide">
              <div className="col-span-2">IMEI</div>
              <div className="col-span-1">Network Provider</div>
              <div className="col-span-1">Updated</div>
            </div>
          </div>

          {initialLoading ? (
            <LoadingSkeleton />
          ) : filteredModems.length === 0 ? (
            <div className="p-12 text-center text-gray-500">
              <Smartphone className="w-12 h-12 text-gray-400 mx-auto mb-4" />
              <h3 className="text-lg font-medium text-gray-900 mb-2">
                {searchTerm ? 'No matching modems found' : 'No modems found'}
              </h3>
              <p className="text-gray-500">
                {searchTerm
                  ? 'Try adjusting your search terms.'
                  : 'No modem information has been tracked yet.'
                }
              </p>
            </div>
          ) : (
            <div className="divide-y divide-gray-200">
              {filteredModems.map((modem) => (
                <div
                  key={modem.id}
                  className="px-4 py-3 hover:bg-gray-50 transition-colors"
                >
                  <div className="grid grid-cols-4 gap-4 items-center">
                    <div className="col-span-2">
                      <div className="flex items-center space-x-3">
                        <Smartphone className="w-4 h-4 text-gray-400 flex-shrink-0" />
                        <div className="min-w-0 flex-1">
                          <code className="text-sm font-mono text-gray-900 block truncate">
                            {modem.imei}
                          </code>
                        </div>
                      </div>
                    </div>

                    <div className="col-span-1">
                      <div className="flex items-center space-x-2">
                        <Signal className="w-4 h-4 text-gray-400 flex-shrink-0" />
                        <div className="text-sm text-gray-600 truncate">
                          {modem.network_provider}
                        </div>
                      </div>
                    </div>

                    <div className="col-span-1">
                      <div className="flex items-center justify-between">
                        <div className="text-sm text-gray-500">
                          {formatTimeAgo(modem.updated_at)}
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </PrivateLayout>
  );
};

export default ModemsPage;
