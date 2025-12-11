'use client';

import React, { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';
import {
  AlertTriangle,
  CheckCircle,
  XCircle,
  Clock,
  Cpu,
  ChevronRight,
  Package,
  AlertCircle,
  Calendar,
  Check,
  X,
} from 'lucide-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import useSmithAPI from "@/app/hooks/smith-api";
import PrivateLayout from "@/app/layouts/PrivateLayout";
import NetworkQualityIndicator from "@/app/components/NetworkQualityIndicator";
import Link from 'next/link';
import { useConfig } from "@/app/hooks/config";

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
  created_at: string;
  updated_at: string;
}

interface Device {
  id: number;
  serial_number: string;
  hostname?: string;
  last_seen: string | null;
  has_token: boolean;
  release_id?: number;
  target_release_id?: number;
  release?: Release;
  target_release?: Release;
  network?: {
    network_score?: number;
  };
  ip_address?: IpAddressInfo;
  system_info?: {
    hostname?: string;
    device_tree?: {
      model?: string;
    };
  };
}

interface Release {
  id: number;
  distribution_id: number;
  distribution_architecture: string;
  distribution_name: string;
  version: string;
  draft: boolean;
  yanked: boolean;
  created_at: string;
}

interface DashboardData {
  total_count: number,
  online_count: number,
  offline_count: number,
  outdated_count: number,
  archived_count: number,
}


const AdminPanel = () => {
  const router = useRouter();
  const { callAPI } = useSmithAPI();
  const { config } = useConfig();
  const queryClient = useQueryClient();
  const [processingDevices, setProcessingDevices] = useState<Set<number>>(new Set());
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  // Build exclude_labels query param
  const excludeLabels = config?.DASHBOARD_EXCLUDED_LABELS
    ?.split(',')
    .map(l => l.trim())
    .filter(Boolean) || [];
  const excludeParams = excludeLabels.length > 0
    ? '&' + excludeLabels.map(l => `exclude_labels=${encodeURIComponent(l)}`).join('&')
    : '';

  const { data: dashboardData, isLoading: dashboardLoading } = useQuery({
    queryKey: ['dashboard'],
    queryFn: () => callAPI<DashboardData>('GET', '/dashboard'),
    refetchInterval: 5000,
  });

  const { data: unapprovedDevices = [], isLoading: unapprovedLoading } = useQuery({
    queryKey: ['devices', 'unapproved'],
    queryFn: () => callAPI<Device[]>('GET', '/devices?approved=false'),
    refetchInterval: 5000,
    select: (data) => data || [],
  });

  const { data: outdatedDevices = [], isLoading: outdatedLoading } = useQuery({
    queryKey: ['devices', 'outdated', excludeParams],
    queryFn: () => callAPI<Device[]>('GET', `/devices?outdated=true&online=true${excludeParams}`),
    refetchInterval: 5000,
    select: (data) => data || [],
  });

  const { data: offlineDevices = [], isLoading: offlineLoading } = useQuery({
    queryKey: ['devices', 'offline', excludeParams],
    queryFn: () => callAPI<Device[]>('GET', `/devices?online=false${excludeParams}`),
    refetchInterval: 5000,
    select: (data) => data || [],
  });

  const loading = dashboardLoading || unapprovedLoading || outdatedLoading || offlineLoading;

  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [toast]);

  const getDeviceStatus = (device: Device) => {
    if (!device.last_seen) return 'never-seen';
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);
    const diffDays = diffMinutes / (60 * 24);
    const isOnline = diffMinutes <= 3;
    
    // Check for stuck update first (takes priority, but only if online)
    if (isOnline && device.release_id && device.target_release_id && device.release_id !== device.target_release_id) {
      return 'stuck-update';
    }
    
    if (isOnline) return 'online';
    if (diffDays <= 1) return 'recently-offline';
    if (diffDays <= 7) return 'offline-week';
    if (diffDays <= 30) return 'offline-month';
    return 'abandoned';
  };

  const getStatusInfo = (status: string) => {
    switch (status) {
      case 'online':
        return { color: 'text-green-600', bgColor: 'bg-green-50 border-green-200', icon: <CheckCircle className="w-4 h-4 text-green-500" />, label: 'Online' };
      case 'stuck-update':
        return { color: 'text-purple-600', bgColor: 'bg-purple-50 border-purple-200', icon: <Package className="w-4 h-4 text-purple-500" />, label: 'Update Failed' };
      case 'recently-offline':
        return { color: 'text-yellow-600', bgColor: 'bg-yellow-50 border-yellow-200', icon: <Clock className="w-4 h-4 text-yellow-500" />, label: 'Recently Offline' };
      case 'offline-week':
        return { color: 'text-orange-600', bgColor: 'bg-orange-50 border-orange-200', icon: <AlertTriangle className="w-4 h-4 text-orange-500" />, label: 'Offline (1 week)' };
      case 'offline-month':
        return { color: 'text-red-600', bgColor: 'bg-red-50 border-red-200', icon: <XCircle className="w-4 h-4 text-red-500" />, label: 'Offline (1 month)' };
      case 'never-seen':
        return { color: 'text-gray-600', bgColor: 'bg-gray-50 border-gray-200', icon: <AlertTriangle className="w-4 h-4 text-gray-500" />, label: 'Never Connected' };
      default:
        return { color: 'text-gray-600', bgColor: 'bg-gray-50 border-gray-200', icon: <XCircle className="w-4 h-4 text-gray-500" />, label: 'Unknown' };
    }
  };

  const getDeviceName = (device: Device) => device.serial_number;

  const formatTimeAgo = (dateString: string) => {
    const now = new Date();
    const past = new Date(dateString);
    const diff = now.getTime() - past.getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d ago`;
    if (hours > 0) return `${hours}h ago`;
    return `${minutes}m ago`;
  };

  const getFlagUrl = (countryCode: string) => {
    return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
  };

  const handleApprove = async (deviceId: number, e: React.MouseEvent) => {
    e.stopPropagation();

    const device = unapprovedDevices.find(d => d.id === deviceId);
    const deviceName = device?.serial_number || 'Device';

    setProcessingDevices(prev => new Set(prev).add(deviceId));

    const success = await callAPI('POST', `/devices/${deviceId}/approval`);

    if (success) {
      queryClient.invalidateQueries({ queryKey: ['devices', 'unapproved'] });
      queryClient.invalidateQueries({ queryKey: ['dashboard'] });
      setToast({
        message: `${deviceName} approved successfully`,
        type: 'success'
      });
    } else {
      setToast({
        message: `Failed to approve ${deviceName}`,
        type: 'error'
      });
    }

    setProcessingDevices(prev => {
      const newSet = new Set(prev);
      newSet.delete(deviceId);
      return newSet;
    });
  };

  const handleReject = async (deviceId: number, e: React.MouseEvent) => {
    e.stopPropagation();

    const device = unapprovedDevices.find(d => d.id === deviceId);
    const deviceName = device?.serial_number || 'Device';

    if (!confirm(`Are you sure you want to reject ${deviceName}? This will archive it.`)) {
      return;
    }

    setProcessingDevices(prev => new Set(prev).add(deviceId));

    const success = await callAPI('DELETE', `/devices/${deviceId}`);

    if (success) {
      queryClient.invalidateQueries({ queryKey: ['devices', 'unapproved'] });
      queryClient.invalidateQueries({ queryKey: ['dashboard'] });
      setToast({
        message: `${deviceName} rejected and archived`,
        type: 'success'
      });
    } else {
      setToast({
        message: `Failed to reject ${deviceName}`,
        type: 'error'
      });
    }

    setProcessingDevices(prev => {
      const newSet = new Set(prev);
      newSet.delete(deviceId);
      return newSet;
    });
  };

  const getDeviceHostname = (device: Device) => {
    return device.system_info?.hostname || 'No hostname';
  };

  // Sort by last_seen descending (most recent first), never seen at the end
  const sortByLastSeen = (a: Device, b: Device) => {
    if (!a.last_seen && !b.last_seen) return 0;
    if (!a.last_seen) return 1;
    if (!b.last_seen) return -1;
    return new Date(b.last_seen).getTime() - new Date(a.last_seen).getTime();
  };

  // Outdated devices (pending update) - sorted by last_seen
  const stuckUpdates = [...outdatedDevices].sort(sortByLastSeen);

  // Offline devices categorized by how long they've been offline
  const categorizeOffline = (devices: Device[]) => {
    const now = new Date();
    const recentlyOffline: Device[] = [];
    const offlineWeek: Device[] = [];
    const offlineMonth: Device[] = [];
    const neverSeen: Device[] = [];

    for (const device of devices) {
      if (!device.last_seen) {
        neverSeen.push(device);
        continue;
      }
      const lastSeen = new Date(device.last_seen);
      const diffDays = (now.getTime() - lastSeen.getTime()) / (1000 * 60 * 60 * 24);

      if (diffDays <= 1) {
        recentlyOffline.push(device);
      } else if (diffDays <= 7) {
        offlineWeek.push(device);
      } else if (diffDays <= 30) {
        offlineMonth.push(device);
      }
      // Devices offline > 30 days are not shown (considered abandoned)
    }

    return {
      recentlyOffline: recentlyOffline.sort(sortByLastSeen),
      offlineWeek: offlineWeek.sort(sortByLastSeen),
      offlineMonth: offlineMonth.sort(sortByLastSeen),
      neverSeen,
    };
  };

  const { recentlyOffline, offlineWeek, offlineMonth, neverSeen } = categorizeOffline(offlineDevices);
  const hasAttentionDevices = stuckUpdates.length > 0 || recentlyOffline.length > 0 || offlineWeek.length > 0 || offlineMonth.length > 0 || neverSeen.length > 0;


  return (
    <PrivateLayout id="dashboard">
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

      <div className="flex gap-6">
        {/* Main Content */}
        <div className="flex-1 space-y-6">

        {/* Overview Stats */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
          {loading ? (
            // Skeleton loading for stats
            <>
              {[...Array(2)].map((_, index) => (
                <div key={index} className="bg-white rounded-lg border border-gray-200 p-6">
                  <div className="flex items-center">
                    <div className="w-8 h-8 bg-gray-200 rounded animate-pulse" />
                    <div className="ml-4">
                      <div className="h-4 bg-gray-200 rounded w-16 animate-pulse mb-2" />
                      <div className="h-8 bg-gray-200 rounded w-12 animate-pulse" />
                    </div>
                  </div>
                </div>
              ))}
            </>
          ) : (
            <>
              <div className="bg-white rounded-lg border border-gray-200 p-6">
                <div className="flex items-center">
                  <CheckCircle className="w-8 h-8 text-green-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Online</p>
                    <p className="text-2xl font-bold text-gray-900">{dashboardData?.online_count || 0}</p>
                  </div>
                </div>
              </div>

              <div className="bg-white rounded-lg border border-gray-200 p-6">
                <div className="flex items-center">
                  <Cpu className="w-8 h-8 text-blue-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Total Devices</p>
                    <p className="text-2xl font-bold text-gray-900">{dashboardData?.total_count || 0}</p>
                  </div>
                </div>
              </div>
            </>
          )}
        </div>

        {/* Device Status Sections */}
        {loading ? (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            {[...Array(4)].map((_, sectionIndex) => (
              <div key={sectionIndex} className="bg-white rounded-lg border border-gray-200">
                <div className="px-4 py-3 border-b border-gray-200">
                  <div className="h-5 bg-gray-200 rounded w-32 animate-pulse" />
                </div>
                <div className="divide-y divide-gray-200">
                  {[...Array(3)].map((_, index) => (
                    <div key={index} className="px-4 py-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center space-x-3 flex-1">
                          <div className="w-4 h-4 bg-gray-200 rounded animate-pulse" />
                          <div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
                        </div>
                        <div className="h-4 bg-gray-200 rounded w-20 animate-pulse" />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        ) : hasAttentionDevices ? (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            {/* Pending Update Section */}
            {stuckUpdates.length > 0 && (
              <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
                <div className="bg-purple-50 px-4 py-3 border-b border-gray-200">
                  <h4 className="text-sm font-semibold text-purple-800 flex items-center">
                    <Package className="w-4 h-4 mr-2" />
                    Pending Update ({stuckUpdates.length})
                  </h4>
                </div>
                <div className="divide-y divide-gray-200">
                  {stuckUpdates.slice(0, 10).map((device) => {
                    const isOnline = device.last_seen ? (new Date().getTime() - new Date(device.last_seen).getTime()) / (1000 * 60) <= 3 : false;
                    return (
                      <Link
                        key={device.id}
                        className="px-4 py-3 hover:bg-purple-50 cursor-pointer transition-colors"
                        href={`/devices/${device.serial_number}`}
                      >
                        <div className="flex items-center justify-between">
                          <div className="flex items-center space-x-3 flex-1">
                            {device.ip_address?.country_code && (
                              <img
                                src={getFlagUrl(device.ip_address.country_code)}
                                alt={device.ip_address.country || 'Country flag'}
                                className="w-4 h-3 flex-shrink-0 rounded-sm"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).style.display = 'none';
                                }}
                              />
                            )}
                            <span className="font-mono text-sm text-gray-900">
                              {getDeviceName(device)}
                            </span>
                            {device.network?.network_score && (
                              <NetworkQualityIndicator
                                isOnline={isOnline}
                                networkScore={device.network.network_score}
                              />
                            )}
                          </div>
                          <div className="flex items-center space-x-3 text-sm">
                            {device.release?.distribution_name && (
                              <span className="text-gray-500">
                                {device.release.distribution_name}
                              </span>
                            )}
                            <span className="text-purple-600 font-mono">
                              {device.release?.version || device.release_id} → {device.target_release?.version || device.target_release_id}
                            </span>
                            <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                          </div>
                        </div>
                      </Link>
                    );
                  })}
                  {stuckUpdates.length > 10 && (
                    <div className="px-4 py-3 bg-gray-50">
                      <button
                        onClick={() => router.push('/devices?outdated=true')}
                        className="text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
                      >
                        View all {stuckUpdates.length} devices →
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* Recently Offline Section */}
            {recentlyOffline.length > 0 && (
              <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
                <div className="bg-yellow-50 px-4 py-3 border-b border-gray-200">
                  <h4 className="text-sm font-semibold text-yellow-800 flex items-center">
                    <Clock className="w-4 h-4 mr-2" />
                    Recently Offline ({recentlyOffline.length})
                  </h4>
                </div>
                <div className="divide-y divide-gray-200">
                  {recentlyOffline.slice(0, 10).map((device) => {
                    return (
                      <Link
                        key={device.id}
                        className="px-4 py-3 hover:bg-yellow-50 cursor-pointer transition-colors"
                        href={`/devices/${device.serial_number}`}
                      >
                        <div className="flex items-center justify-between">
                          <div className="flex items-center space-x-3 flex-1">
                            {device.ip_address?.country_code && (
                              <img
                                src={getFlagUrl(device.ip_address.country_code)}
                                alt={device.ip_address.country || 'Country flag'}
                                className="w-4 h-3 flex-shrink-0 rounded-sm"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).style.display = 'none';
                                }}
                              />
                            )}
                            <span className="font-mono text-sm text-gray-900">
                              {getDeviceName(device)}
                            </span>
                          </div>
                          <div className="flex items-center space-x-3 text-sm">
                            <span className="text-gray-500">
                              {device.last_seen ? formatTimeAgo(device.last_seen) : 'never'}
                            </span>
                            <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                          </div>
                        </div>
                      </Link>
                    );
                  })}
                  {recentlyOffline.length > 10 && (
                    <div className="px-4 py-3 bg-gray-50">
                      <button
                        onClick={() => router.push('/devices')}
                        className="text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
                      >
                        View all {recentlyOffline.length} devices →
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* Offline This Week Section */}
            {offlineWeek.length > 0 && (
              <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
                <div className="bg-orange-50 px-4 py-3 border-b border-gray-200">
                  <h4 className="text-sm font-semibold text-orange-800 flex items-center">
                    <AlertTriangle className="w-4 h-4 mr-2" />
                    Offline This Week ({offlineWeek.length})
                  </h4>
                </div>
                <div className="divide-y divide-gray-200">
                  {offlineWeek.slice(0, 10).map((device) => {
                    return (
                      <Link
                        key={device.id}
                        className="px-4 py-3 hover:bg-orange-50 cursor-pointer transition-colors"
                        href={`/devices/${device.serial_number}`}
                      >
                        <div className="flex items-center justify-between">
                          <div className="flex items-center space-x-3 flex-1">
                            {device.ip_address?.country_code && (
                              <img
                                src={getFlagUrl(device.ip_address.country_code)}
                                alt={device.ip_address.country || 'Country flag'}
                                className="w-4 h-3 flex-shrink-0 rounded-sm"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).style.display = 'none';
                                }}
                              />
                            )}
                            <span className="font-mono text-sm text-gray-900">
                              {getDeviceName(device)}
                            </span>
                          </div>
                          <div className="flex items-center space-x-3 text-sm">
                            <span className="text-gray-500">
                              {device.last_seen ? formatTimeAgo(device.last_seen) : 'never'}
                            </span>
                            <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                          </div>
                        </div>
                      </Link>
                    );
                  })}
                  {offlineWeek.length > 10 && (
                    <div className="px-4 py-3 bg-gray-50">
                      <button
                        onClick={() => router.push('/devices')}
                        className="text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
                      >
                        View all {offlineWeek.length} devices →
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* Long-term Offline Section */}
            {offlineMonth.length > 0 && (
              <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
                <div className="bg-red-50 px-4 py-3 border-b border-gray-200">
                  <h4 className="text-sm font-semibold text-red-800 flex items-center">
                    <XCircle className="w-4 h-4 mr-2" />
                    Long-term Offline ({offlineMonth.length})
                  </h4>
                </div>
                <div className="divide-y divide-gray-200">
                  {offlineMonth.slice(0, 10).map((device) => {
                    return (
                      <Link
                        key={device.id}
                        className="px-4 py-3 hover:bg-red-50 cursor-pointer transition-colors"
                        href={`/devices/${device.serial_number}`}
                      >
                        <div className="flex items-center justify-between">
                          <div className="flex items-center space-x-3 flex-1">
                            {device.ip_address?.country_code && (
                              <img
                                src={getFlagUrl(device.ip_address.country_code)}
                                alt={device.ip_address.country || 'Country flag'}
                                className="w-4 h-3 flex-shrink-0 rounded-sm"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).style.display = 'none';
                                }}
                              />
                            )}
                            <span className="font-mono text-sm text-gray-900">
                              {getDeviceName(device)}
                            </span>
                          </div>
                          <div className="flex items-center space-x-3 text-sm">
                            <span className="text-gray-500">
                              {device.last_seen ? formatTimeAgo(device.last_seen) : 'never'}
                            </span>
                            <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                          </div>
                        </div>
                      </Link>
                    );
                  })}
                  {offlineMonth.length > 10 && (
                    <div className="px-4 py-3 bg-gray-50">
                      <button
                        onClick={() => router.push('/devices')}
                        className="text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
                      >
                        View all {offlineMonth.length} devices →
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* Never Connected Section */}
            {neverSeen.length > 0 && (
              <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
                <div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
                  <h4 className="text-sm font-semibold text-gray-800 flex items-center">
                    <AlertTriangle className="w-4 h-4 mr-2" />
                    Never Connected ({neverSeen.length})
                  </h4>
                </div>
                <div className="divide-y divide-gray-200">
                  {neverSeen.slice(0, 10).map((device) => {
                    return (
                      <div
                        key={device.id}
                        className="px-4 py-3 hover:bg-gray-50 cursor-pointer transition-colors"
                        onClick={() => router.push(`/devices/${device.serial_number}`)}
                      >
                        <div className="flex items-center justify-between">
                          <div className="flex items-center space-x-3 flex-1">
                            {device.ip_address?.country_code && (
                              <img
                                src={getFlagUrl(device.ip_address.country_code)}
                                alt={device.ip_address.country || 'Country flag'}
                                className="w-4 h-3 flex-shrink-0 rounded-sm"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).style.display = 'none';
                                }}
                              />
                            )}
                            <span className="font-mono text-sm text-gray-900">
                              {getDeviceName(device)}
                            </span>
                          </div>
                          <div className="flex items-center space-x-3 text-sm">
                            <span className="text-gray-500">Never</span>
                            <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                          </div>
                        </div>
                      </div>
                    );
                  })}
                  {neverSeen.length > 10 && (
                    <div className="px-4 py-3 bg-gray-50">
                      <button
                        onClick={() => router.push('/devices')}
                        className="text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
                      >
                        View all {neverSeen.length} devices →
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        ) : (
          /* All Good Message */
          <div className="bg-green-50 border border-green-200 rounded-lg p-6">
            <div className="flex items-center space-x-3">
              <CheckCircle className="w-6 h-6 text-green-500" />
              <div>
                <h3 className="text-lg font-semibold text-green-900">All Systems Operational</h3>
                <p className="text-sm text-green-700 mt-1">
                  No devices need attention. All updates are successful and devices are either online or archived (offline >30 days).
                </p>
              </div>
            </div>
          </div>
        )}
        </div>

        {/* Right Sidebar - Unapproved Devices */}
        <div className="hidden xl:block w-96 space-y-6">
          <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
            <div className="bg-orange-50 px-4 py-3 border-b border-gray-200">
              <div className="flex items-center justify-between">
                <h4 className="text-sm font-semibold text-orange-800">Pending Approval</h4>
                {!loading && unapprovedDevices.length > 0 && (
                  <span className="text-xs text-orange-600">{unapprovedDevices.length} device{unapprovedDevices.length !== 1 ? 's' : ''}</span>
                )}
              </div>
            </div>
            <div className="max-h-[calc(100vh-200px)] overflow-y-auto">
              {loading ? (
                <div className="p-4 space-y-3">
                  {[...Array(3)].map((_, i) => (
                    <div key={i} className="animate-pulse">
                      <div className="h-4 bg-gray-200 rounded w-3/4 mb-2"></div>
                      <div className="h-3 bg-gray-200 rounded w-1/2"></div>
                    </div>
                  ))}
                </div>
              ) : unapprovedDevices.length === 0 ? (
                <div className="p-6 text-center">
                  <CheckCircle className="w-10 h-10 text-green-500 mx-auto mb-2" />
                  <p className="text-sm font-medium text-gray-900 mb-1">All caught up!</p>
                  <p className="text-xs text-gray-600">No devices pending approval</p>
                </div>
              ) : (
                <div className="divide-y divide-gray-200">
                  {unapprovedDevices.map((device) => (
                    <div key={device.id} className="p-4 hover:bg-gray-50">
                      <div className="flex items-start justify-between mb-2">
                        <div className="flex items-center space-x-2 min-w-0 flex-1">
                          <AlertCircle className="w-4 h-4 text-orange-500 flex-shrink-0" />
                          <div className="min-w-0">
                            <p className="text-sm font-medium text-gray-900 truncate">
                              {device.serial_number}
                            </p>
                            <p className="text-xs text-gray-500 truncate">
                              {getDeviceHostname(device)}
                            </p>
                          </div>
                        </div>
                      </div>
                      <div className="flex items-center space-x-1 text-xs text-gray-500 mb-3">
                        <Calendar className="w-3 h-3" />
                        <span>{formatTimeAgo(device.created_on)}</span>
                      </div>
                      <div className="flex space-x-2">
                        <button
                          onClick={(e) => handleApprove(device.id, e)}
                          disabled={processingDevices.has(device.id)}
                          className="flex-1 flex items-center justify-center space-x-1 px-3 py-1.5 text-xs font-medium text-white bg-green-600 hover:bg-green-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded transition-colors"
                        >
                          <CheckCircle className="w-3 h-3" />
                          <span>Approve</span>
                        </button>
                        <button
                          onClick={(e) => handleReject(device.id, e)}
                          disabled={processingDevices.has(device.id)}
                          className="flex-1 flex items-center justify-center space-x-1 px-3 py-1.5 text-xs font-medium text-white bg-red-600 hover:bg-red-700 disabled:bg-gray-300 disabled:cursor-not-allowed rounded transition-colors"
                        >
                          <XCircle className="w-3 h-3" />
                          <span>Reject</span>
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default AdminPanel;
