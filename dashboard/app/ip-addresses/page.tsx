'use client';

import React, { useState, useEffect } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import {
  Globe,
  Search,
  Shield,
  Wifi,
  MapPin,
  Building,
  Clock,
  Calendar,
  Edit2,
  Check,
  X,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

interface IpAddressInfo {
  id: number;
  ip_address: string;
  name?: string;
  continent?: string;
  continent_code?: string;
  country_code?: string;
  country?: string;
  region?: string;
  city?: string;
  isp?: string;
  coordinates?: [number, number];
  proxy?: boolean;
  hosting?: boolean;
  device_count?: number;
  created_at: string;
  updated_at: string;
}

interface IpAddressListResponse {
  ip_addresses: IpAddressInfo[];
}

const IpAddressSkeleton = () => (
  <div className="px-4 py-3 animate-pulse">
    <div className="grid grid-cols-6 gap-4 items-center">
      <div className="col-span-2">
        <div className="flex items-center space-x-3">
          <div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0"></div>
          <div className="space-y-1">
            <div className="h-4 bg-gray-300 rounded w-32"></div>
            <div className="h-3 bg-gray-200 rounded w-20"></div>
          </div>
        </div>
      </div>
      <div className="col-span-2">
        <div className="flex items-center space-x-2">
          <div className="w-4 h-3 bg-gray-300 rounded-sm flex-shrink-0"></div>
          <div className="h-4 bg-gray-300 rounded w-24"></div>
        </div>
      </div>
      <div className="col-span-1">
        <div className="h-4 bg-gray-300 rounded w-20"></div>
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
      <IpAddressSkeleton key={i} />
    ))}
  </div>
);

const IpAddressesPage = () => {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { callAPI, loading } = useSmithAPI();
  const [ipAddresses, setIpAddresses] = useState<IpAddressInfo[]>([]);
  const [filteredIpAddresses, setFilteredIpAddresses] = useState<IpAddressInfo[]>([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingName, setEditingName] = useState('');
  const [saving, setSaving] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  // Initialize search term from URL params
  useEffect(() => {
    const urlSearch = searchParams.get('search') || '';
    setSearchTerm(urlSearch);
  }, [searchParams]);

  useEffect(() => {
    const fetchIpAddresses = async () => {
      try {
        const response = await callAPI<IpAddressListResponse>('GET', '/ip_addresses');
        if (response?.ip_addresses) {
          setIpAddresses(response.ip_addresses);
        }
      } finally {
        setInitialLoading(false);
      }
    };
    fetchIpAddresses();
  }, [callAPI]);

  useEffect(() => {
    if (searchTerm === '') {
      setFilteredIpAddresses(ipAddresses);
    } else {
      const filtered = ipAddresses.filter(ip => 
        ip.ip_address.toLowerCase().includes(searchTerm.toLowerCase()) ||
        ip.name?.toLowerCase().includes(searchTerm.toLowerCase()) ||
        ip.country?.toLowerCase().includes(searchTerm.toLowerCase()) ||
        ip.city?.toLowerCase().includes(searchTerm.toLowerCase()) ||
        ip.isp?.toLowerCase().includes(searchTerm.toLowerCase())
      );
      setFilteredIpAddresses(filtered);
    }
  }, [searchTerm, ipAddresses]);

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

  const getFlagUrl = (countryCode: string) => {
    return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
  };

  const getLocationString = (ip: IpAddressInfo) => {
    const parts = [];
    if (ip.city) parts.push(ip.city);
    if (ip.region && ip.region !== ip.city) parts.push(ip.region);
    if (ip.country) parts.push(ip.country);
    return parts.join(', ') || 'Unknown Location';
  };

  const updateSearchInUrl = (newSearchTerm: string) => {
    const params = new URLSearchParams(searchParams);
    if (newSearchTerm) {
      params.set('search', newSearchTerm);
    } else {
      params.delete('search');
    }
    router.push(`/ip-addresses?${params.toString()}`, { scroll: false });
  };

  const handleSearchChange = (value: string) => {
    setSearchTerm(value);
    updateSearchInUrl(value);
  };

  const startEditing = (ip: IpAddressInfo) => {
    setEditingId(ip.id);
    setEditingName(ip.name || '');
  };

  const cancelEditing = () => {
    setEditingId(null);
    setEditingName('');
  };

  const saveEdit = async () => {
    if (editingId === null || saving) return;

    setSaving(true);
    try {
      const requestBody = {
        name: editingName.trim() || null,
      };

      console.log('Saving IP address:', editingId, requestBody);

      const updatedIp = await callAPI<IpAddressInfo>('PUT', `/ip_address/${editingId}`, requestBody);

      if (updatedIp) {
        // Update both arrays
        setIpAddresses(prev => prev.map(ip => ip.id === editingId ? updatedIp : ip));
        setFilteredIpAddresses(prev => prev.map(ip => ip.id === editingId ? updatedIp : ip));
        
        // Show success toast
        const name = editingName.trim();
        setToast({ 
          message: name 
            ? `IP address renamed to "${name}"` 
            : 'IP address name cleared',
          type: 'success' 
        });
      }

      setEditingId(null);
      setEditingName('');
    } catch (error: any) {
      console.error('Failed to update IP address name:', error);
      setToast({ 
        message: `Failed to update: ${error?.message || 'Unknown error'}`,
        type: 'error' 
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <PrivateLayout id="ip-addresses">
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
        {/* Search and IP Count */}
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center space-x-4">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
              <input
                type="text"
                placeholder="Search IP addresses..."
                value={searchTerm}
                onChange={(e) => handleSearchChange(e.target.value)}
                className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
              />
            </div>
          </div>

          <div className="mt-4 sm:mt-0 flex items-center space-x-3">
            <span className="text-sm text-gray-500">
              {initialLoading ? 'Loading...' : `${filteredIpAddresses.length} IP address${filteredIpAddresses.length !== 1 ? 'es' : ''} shown`}
            </span>
          </div>
        </div>

        {/* IP Address List */}
        <div className="border border-gray-200 rounded-lg overflow-hidden bg-white">
          <div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
            <div className="grid grid-cols-6 gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide">
              <div className="col-span-2">IP Address / Name</div>
              <div className="col-span-2">Location</div>
              <div className="col-span-1">ISP</div>
              <div className="col-span-1">Updated</div>
            </div>
          </div>

          {initialLoading ? (
            <LoadingSkeleton />
          ) : filteredIpAddresses.length === 0 ? (
            <div className="p-12 text-center text-gray-500">
              <Globe className="w-12 h-12 text-gray-400 mx-auto mb-4" />
              <h3 className="text-lg font-medium text-gray-900 mb-2">
                {searchTerm ? 'No matching IP addresses found' : 'No IP addresses found'}
              </h3>
              <p className="text-gray-500">
                {searchTerm 
                  ? 'Try adjusting your search terms.'
                  : 'No IP address information has been tracked yet.'
                }
              </p>
            </div>
          ) : (
            <div className="divide-y divide-gray-200">
              {filteredIpAddresses.map((ipInfo) => (
                <div 
                  key={ipInfo.id} 
                  className="px-4 py-3 hover:bg-gray-50 transition-colors"
                >
                  <div className="grid grid-cols-6 gap-4 items-center">
                    <div className="col-span-2">
                      <div className="flex items-center space-x-3">
                        <Globe className="w-4 h-4 text-gray-400 flex-shrink-0" />
                        <div className="min-w-0 flex-1">
                          {editingId === ipInfo.id ? (
                            <div className="flex items-center space-x-2">
                              <input
                                type="text"
                                value={editingName}
                                onChange={(e) => setEditingName(e.target.value)}
                                placeholder={ipInfo.ip_address}
                                disabled={saving}
                                className={`text-sm font-medium text-gray-900 bg-white border border-gray-300 rounded px-2 py-1 min-w-0 flex-1 ${
                                  saving ? 'opacity-50 cursor-not-allowed' : ''
                                }`}
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter' && !saving) saveEdit();
                                  if (e.key === 'Escape' && !saving) cancelEditing();
                                }}
                                autoFocus
                              />
                              <button
                                onClick={saveEdit}
                                disabled={saving}
                                className={`p-1 transition-colors ${
                                  saving 
                                    ? 'text-gray-400 cursor-not-allowed' 
                                    : 'text-green-600 hover:text-green-800'
                                }`}
                              >
                                {saving ? (
                                  <div className="w-4 h-4 border-2 border-green-600 border-t-transparent rounded-full animate-spin"></div>
                                ) : (
                                  <Check className="w-4 h-4" />
                                )}
                              </button>
                              <button
                                onClick={cancelEditing}
                                disabled={saving}
                                className={`p-1 transition-colors ${
                                  saving 
                                    ? 'text-gray-400 cursor-not-allowed' 
                                    : 'text-red-600 hover:text-red-800'
                                }`}
                              >
                                <X className="w-4 h-4" />
                              </button>
                            </div>
                          ) : (
                            <div className="flex items-center space-x-2 mb-1">
                              {ipInfo.name ? (
                                <div className="text-sm font-medium text-gray-900 truncate">
                                  {ipInfo.name}
                                </div>
                              ) : (
                                <code className="text-sm font-mono text-gray-900">
                                  {ipInfo.ip_address}
                                </code>
                              )}
                              {ipInfo.proxy && (
                                <span className="px-1.5 py-0.5 text-xs font-medium bg-orange-100 text-orange-800 rounded-full flex-shrink-0">
                                  <Shield className="w-2 h-2 inline mr-1" />
                                  Proxy
                                </span>
                              )}
                              {ipInfo.hosting && (
                                <span className="px-1.5 py-0.5 text-xs font-medium bg-purple-100 text-purple-800 rounded-full flex-shrink-0">
                                  <Wifi className="w-2 h-2 inline mr-1" />
                                  Host
                                </span>
                              )}
                              {(ipInfo.device_count || 0) > 0 && (
                                <span className="px-1.5 py-0.5 text-xs font-medium bg-blue-100 text-blue-800 rounded-full flex-shrink-0">
                                  {ipInfo.device_count} device{ipInfo.device_count !== 1 ? 's' : ''}
                                </span>
                              )}
                            </div>
                          )}
                          {!editingId || editingId !== ipInfo.id ? (
                            ipInfo.name && (
                              <code className="text-xs font-mono text-gray-600">
                                {ipInfo.ip_address}
                              </code>
                            )
                          ) : null}
                        </div>
                      </div>
                    </div>

                    <div className="col-span-2">
                      <div className="flex items-center space-x-2">
                        {ipInfo.country_code && (
                          <img 
                            src={getFlagUrl(ipInfo.country_code)} 
                            alt={ipInfo.country || 'Country flag'} 
                            className="w-4 h-3 flex-shrink-0 rounded-sm"
                            onError={(e) => {
                              (e.target as HTMLImageElement).style.display = 'none';
                            }}
                          />
                        )}
                        <div className="text-sm text-gray-600 truncate">
                          {getLocationString(ipInfo)}
                        </div>
                      </div>
                    </div>

                    <div className="col-span-1 text-sm text-gray-600 truncate">
                      {ipInfo.isp || 'Unknown'}
                    </div>

                    <div className="col-span-1">
                      <div className="flex items-center justify-between">
                        <div className="text-sm text-gray-500">
                          {formatTimeAgo(ipInfo.updated_at)}
                        </div>
                        {editingId !== ipInfo.id && (
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              startEditing(ipInfo);
                            }}
                            className="p-1 text-gray-400 hover:text-gray-600 transition-colors"
                            title="Edit name"
                          >
                            <Edit2 className="w-4 h-4" />
                          </button>
                        )}
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

export default IpAddressesPage;