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
} from 'lucide-react';
import useSmithAPI from "@/app/hooks/smith-api";
import PrivateLayout from "@/app/layouts/PrivateLayout";

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
  const [dashboardData, setDashboardData] = useState<DashboardData | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [attentionDevices, setAttentionDevices] = useState<Device[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchData = async () => {
      setLoading(true);
      try {
        // Fetch dashboard stats
        const dashData = await callAPI<DashboardData>('GET', '/dashboard');
        if (dashData) {
          setDashboardData(dashData);
        }

        // Fetch devices for attention analysis
        const devicesData = await callAPI<Device[]>('GET', '/devices');
        if (devicesData) {
          const authorizedDevices = devicesData.filter(device => device.has_token);
          setDevices(authorizedDevices);
          
          // Filter devices that need attention
          const needsAttention = authorizedDevices.filter(device => {
            // Never seen = needs attention
            if (!device.last_seen) return true;
            
            const lastSeen = new Date(device.last_seen);
            const now = new Date();
            const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);
            const daysSinceLastSeen = diffMinutes / (60 * 24);
            const isOnline = diffMinutes <= 3;
            
            // Stuck on update = needs attention (but only if device is online)
            if (isOnline && device.release_id && device.target_release_id && device.release_id !== device.target_release_id) {
              return true;
            }
            
            // Offline for more than 3 minutes but less than 30 days
            return daysSinceLastSeen > (3 / (24 * 60)) && daysSinceLastSeen <= 30;
          });
          
          setAttentionDevices(needsAttention);
        }

      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, [callAPI]);

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

  // Categorize attention devices
  const stuckUpdates = attentionDevices.filter(d => getDeviceStatus(d) === 'stuck-update');
  const recentlyOffline = attentionDevices.filter(d => getDeviceStatus(d) === 'recently-offline');
  const offlineWeek = attentionDevices.filter(d => getDeviceStatus(d) === 'offline-week');
  const offlineMonth = attentionDevices.filter(d => getDeviceStatus(d) === 'offline-month');
  const neverSeen = attentionDevices.filter(d => getDeviceStatus(d) === 'never-seen');


  return (
    <PrivateLayout id="dashboard">
      <div className="space-y-6">
        {/* Overview Stats */}
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-6">
          {loading ? (
            // Skeleton loading for stats
            <>
              {[...Array(4)].map((_, index) => (
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
                  <AlertTriangle className="w-8 h-8 text-red-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Need Attention</p>
                    <p className="text-2xl font-bold text-gray-900">{attentionDevices.length}</p>
                  </div>
                </div>
              </div>

              <div
                className="bg-white rounded-lg border border-gray-200 p-6 hover:bg-gray-50 cursor-pointer transition-colors"
                onClick={() => router.push('/devices?outdated=true')}
              >
                <div className="flex items-center">
                  <Package className="w-8 h-8 text-yellow-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Outdated</p>
                    <p className="text-2xl font-bold text-gray-900">{dashboardData?.outdated_count || 0}</p>
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

        {/* Devices Needing Attention */}
        {loading ? (
          // Skeleton loading for attention section
          <div className="bg-white rounded-lg border border-gray-200">
            <div className="p-6 border-b border-gray-200">
              <div className="flex items-center space-x-3">
                <div className="w-6 h-6 bg-gray-200 rounded animate-pulse" />
                <div>
                  <div className="h-6 bg-gray-200 rounded w-48 animate-pulse mb-2" />
                  <div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
                </div>
              </div>
            </div>
            <div className="p-6">
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                {[...Array(6)].map((_, index) => (
                  <div key={index} className="border border-gray-200 rounded-lg p-4">
                    <div className="flex items-center justify-between mb-2">
                      <div className="flex items-center space-x-2">
                        <div className="w-4 h-4 bg-gray-200 rounded animate-pulse" />
                        <div className="h-4 bg-gray-200 rounded w-24 animate-pulse" />
                      </div>
                      <div className="w-4 h-4 bg-gray-200 rounded animate-pulse" />
                    </div>
                    <div className="h-3 bg-gray-200 rounded w-32 animate-pulse mb-2" />
                    <div className="h-3 bg-gray-200 rounded w-20 animate-pulse" />
                  </div>
                ))}
              </div>
            </div>
          </div>
        ) : attentionDevices.length > 0 ? (
          <div className="bg-white rounded-lg border border-red-200">
            <div className="p-6 border-b border-gray-200 bg-red-50">
              <div className="flex items-center space-x-3">
                <AlertTriangle className="w-6 h-6 text-red-500" />
                <div>
                  <h3 className="text-lg font-semibold text-red-900">Devices Needing Attention</h3>
                  <p className="text-sm text-red-700 mt-1">
                    {attentionDevices.length} device{attentionDevices.length > 1 ? 's' : ''} with failed updates or offline issues
                  </p>
                </div>
              </div>
            </div>
            
            <div className="p-6 space-y-6">
              {/* Critical - Stuck Updates */}
              {stuckUpdates.length > 0 && (
                <div>
                  <h4 className="text-md font-semibold text-purple-800 mb-3 flex items-center">
                    <Package className="w-4 h-4 mr-2" />
                    Update Failed ({stuckUpdates.length})
                  </h4>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
                    {stuckUpdates.slice(0, 8).map((device) => {
                      const status = getDeviceStatus(device);
                      const statusInfo = getStatusInfo(status);
                      return (
                        <div 
                          key={device.id}
                          className="border border-purple-200 rounded-lg p-3 hover:bg-purple-50 cursor-pointer transition-colors"
                          onClick={() => router.push(`/devices/${device.serial_number}`)}
                        >
                          <div className="flex items-center justify-between mb-1">
                            <div className="flex items-center space-x-2">
                              {statusInfo.icon}
                              <span className="font-mono text-xs font-semibold text-gray-900 truncate">
                                {getDeviceName(device)}
                              </span>
                            </div>
                            <ChevronRight className="w-3 h-3 text-gray-400 flex-shrink-0" />
                          </div>
                          <p className="text-xs text-purple-600">
                            {device.release?.version || device.release_id} → {device.target_release?.version || device.target_release_id}
                          </p>
                        </div>
                      );
                    })}
                  </div>
                  {stuckUpdates.length > 8 && (
                    <button
                      onClick={() => router.push('/devices?outdated=true')}
                      className="text-sm text-blue-600 hover:text-blue-800 mt-3"
                    >
                      View all {stuckUpdates.length} devices with update issues →
                    </button>
                  )}
                </div>
              )}

              {/* Critical - Recently Offline (< 1 day) */}
              {recentlyOffline.length > 0 && (
                <div>
                  <h4 className="text-md font-semibold text-yellow-800 mb-3 flex items-center">
                    <Clock className="w-4 h-4 mr-2" />
                    Recently Offline ({recentlyOffline.length})
                  </h4>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
                    {recentlyOffline.slice(0, 8).map((device) => {
                      const status = getDeviceStatus(device);
                      const statusInfo = getStatusInfo(status);
                      return (
                        <div 
                          key={device.id}
                          className="border border-yellow-200 rounded-lg p-3 hover:bg-yellow-50 cursor-pointer transition-colors"
                          onClick={() => router.push(`/devices/${device.serial_number}`)}
                        >
                          <div className="flex items-center justify-between mb-1">
                            <div className="flex items-center space-x-2">
                              {statusInfo.icon}
                              <span className="font-mono text-xs font-semibold text-gray-900 truncate">
                                {getDeviceName(device)}
                              </span>
                            </div>
                            <ChevronRight className="w-3 h-3 text-gray-400 flex-shrink-0" />
                          </div>
                          <p className="text-xs text-yellow-600">
                            {device.last_seen ? formatTimeAgo(device.last_seen) : 'Never'}
                          </p>
                        </div>
                      );
                    })}
                  </div>
                  {recentlyOffline.length > 8 && (
                    <button 
                      onClick={() => router.push('/devices')}
                      className="text-sm text-blue-600 hover:text-blue-800 mt-3"
                    >
                      View all {recentlyOffline.length} recently offline devices →
                    </button>
                  )}
                </div>
              )}

              {/* Concerning - Offline for 1 week */}
              {offlineWeek.length > 0 && (
                <div>
                  <h4 className="text-md font-semibold text-orange-800 mb-3 flex items-center">
                    <AlertTriangle className="w-4 h-4 mr-2" />
                    Offline This Week ({offlineWeek.length})
                  </h4>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
                    {offlineWeek.slice(0, 8).map((device) => {
                      const status = getDeviceStatus(device);
                      const statusInfo = getStatusInfo(status);
                      return (
                        <div 
                          key={device.id}
                          className="border border-orange-200 rounded-lg p-3 hover:bg-orange-50 cursor-pointer transition-colors"
                          onClick={() => router.push(`/devices/${device.serial_number}`)}
                        >
                          <div className="flex items-center justify-between mb-1">
                            <div className="flex items-center space-x-2">
                              {statusInfo.icon}
                              <span className="font-mono text-xs font-semibold text-gray-900 truncate">
                                {getDeviceName(device)}
                              </span>
                            </div>
                            <ChevronRight className="w-3 h-3 text-gray-400 flex-shrink-0" />
                          </div>
                          <p className="text-xs text-orange-600">
                            {device.last_seen ? formatTimeAgo(device.last_seen) : 'Never'}
                          </p>
                        </div>
                      );
                    })}
                  </div>
                  {offlineWeek.length > 8 && (
                    <button 
                      onClick={() => router.push('/devices')}
                      className="text-sm text-blue-600 hover:text-blue-800 mt-3"
                    >
                      View all {offlineWeek.length} devices offline this week →
                    </button>
                  )}
                </div>
              )}

              {/* Critical - Offline for 1 month */}
              {offlineMonth.length > 0 && (
                <div>
                  <h4 className="text-md font-semibold text-red-800 mb-3 flex items-center">
                    <XCircle className="w-4 h-4 mr-2" />
                    Long-term Offline ({offlineMonth.length})
                  </h4>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
                    {offlineMonth.slice(0, 8).map((device) => {
                      const status = getDeviceStatus(device);
                      const statusInfo = getStatusInfo(status);
                      return (
                        <div 
                          key={device.id}
                          className="border border-red-200 rounded-lg p-3 hover:bg-red-50 cursor-pointer transition-colors"
                          onClick={() => router.push(`/devices/${device.serial_number}`)}
                        >
                          <div className="flex items-center justify-between mb-1">
                            <div className="flex items-center space-x-2">
                              {statusInfo.icon}
                              <span className="font-mono text-xs font-semibold text-gray-900 truncate">
                                {getDeviceName(device)}
                              </span>
                            </div>
                            <ChevronRight className="w-3 h-3 text-gray-400 flex-shrink-0" />
                          </div>
                          <p className="text-xs text-red-600">
                            {device.last_seen ? formatTimeAgo(device.last_seen) : 'Never'}
                          </p>
                        </div>
                      );
                    })}
                  </div>
                  {offlineMonth.length > 8 && (
                    <button 
                      onClick={() => router.push('/devices')}
                      className="text-sm text-blue-600 hover:text-blue-800 mt-3"
                    >
                      View all {offlineMonth.length} long-term offline devices →
                    </button>
                  )}
                </div>
              )}

              {/* Never Connected */}
              {neverSeen.length > 0 && (
                <div>
                  <h4 className="text-md font-semibold text-gray-800 mb-3 flex items-center">
                    <AlertTriangle className="w-4 h-4 mr-2" />
                    Never Connected ({neverSeen.length})
                  </h4>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
                    {neverSeen.slice(0, 8).map((device) => {
                      const status = getDeviceStatus(device);
                      const statusInfo = getStatusInfo(status);
                      return (
                        <div 
                          key={device.id}
                          className="border border-gray-200 rounded-lg p-3 hover:bg-gray-50 cursor-pointer transition-colors"
                          onClick={() => router.push(`/devices/${device.serial_number}`)}
                        >
                          <div className="flex items-center justify-between mb-1">
                            <div className="flex items-center space-x-2">
                              {statusInfo.icon}
                              <span className="font-mono text-xs font-semibold text-gray-900 truncate">
                                {getDeviceName(device)}
                              </span>
                            </div>
                            <ChevronRight className="w-3 h-3 text-gray-400 flex-shrink-0" />
                          </div>
                          <p className="text-xs text-gray-600">Never connected</p>
                        </div>
                      );
                    })}
                  </div>
                  {neverSeen.length > 8 && (
                    <button 
                      onClick={() => router.push('/devices')}
                      className="text-sm text-blue-600 hover:text-blue-800 mt-3"
                    >
                      View all {neverSeen.length} devices that never connected →
                    </button>
                  )}
                </div>
              )}
            </div>
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
    </PrivateLayout>
  );
};

export default AdminPanel;
