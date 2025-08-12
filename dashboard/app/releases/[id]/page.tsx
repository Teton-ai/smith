'use client';

import React, { useState, useEffect } from 'react';
import { useRouter, useParams } from 'next/navigation';
import {
  Package,
  Tag,
  Download,
  Calendar,
  ArrowLeft,
  Users,
  Box,
  Cpu,
  Monitor,
  HardDrive,
  CheckCircle,
  XCircle,
  Clock,
  AlertTriangle,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

interface Release {
  id: number;
  version: string;
  distribution_id: number;
  created_at: string;
  size?: number;
  download_count?: number;
  yanked?: boolean;
  draft?: boolean;
}

interface Distribution {
  id: number;
  name: string;
  description: string | null;
  architecture: string;
  num_packages: number | null;
}

interface ReleasePackage {
  id: number;
  name: string;
  version: string;
  description?: string;
  size?: number;
  checksum?: string;
}

interface Device {
  id: number;
  serial_number: string;
  hostname?: string;
  last_seen: string | null;
  has_token: boolean;
  release_id?: number;
  target_release_id?: number;
  system_info?: {
    hostname?: string;
    device_tree?: {
      model?: string;
    };
  };
}

type TabType = 'packages' | 'devices';

const ReleaseDetailPage = () => {
  const router = useRouter();
  const params = useParams();
  const releaseId = params.id as string;
  const { callAPI, loading, error } = useSmithAPI();
  const [release, setRelease] = useState<Release | null>(null);
  const [distribution, setDistribution] = useState<Distribution | null>(null);
  const [packages, setPackages] = useState<ReleasePackage[]>([]);
  const [devices, setDevices] = useState<Device[]>([]);
  const [activeTab, setActiveTab] = useState<TabType>('packages');
  const [packagesLoading, setPackagesLoading] = useState(false);
  const [devicesLoading, setDevicesLoading] = useState(false);

  useEffect(() => {
    const fetchRelease = async () => {
      const data = await callAPI<Release>('GET', `/releases/${releaseId}`);
      if (data) {
        setRelease(data);
        
        // Fetch distribution info
        const distData = await callAPI<Distribution>('GET', `/distributions/${data.distribution_id}`);
        if (distData) {
          setDistribution(distData);
        }
      }
    };
    if (releaseId) {
      fetchRelease();
    }
  }, [releaseId, callAPI]);

  useEffect(() => {
    if (activeTab === 'packages' && releaseId) {
      const fetchPackages = async () => {
        setPackagesLoading(true);
        try {
          const data = await callAPI<ReleasePackage[]>('GET', `/releases/${releaseId}/packages`);
          if (data) {
            setPackages(data);
          }
        } finally {
          setPackagesLoading(false);
        }
      };
      fetchPackages();
    }
  }, [activeTab, releaseId, callAPI]);

  useEffect(() => {
    if (activeTab === 'devices' && releaseId) {
      const fetchDevices = async () => {
        setDevicesLoading(true);
        try {
          const data = await callAPI<Device[]>('GET', `/releases/${releaseId}/devices`);
          if (data) {
            setDevices(data);
          }
        } finally {
          setDevicesLoading(false);
        }
      };
      fetchDevices();
    }
  }, [activeTab, releaseId, callAPI]);

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const formatFileSize = (bytes?: number) => {
    if (!bytes) return 'Unknown';
    const mb = bytes / (1024 * 1024);
    if (mb < 1024) {
      return `${mb.toFixed(1)} MB`;
    }
    const gb = mb / 1024;
    return `${gb.toFixed(1)} GB`;
  };

  const getDeviceStatus = (device: Device) => {
    if (!device.last_seen) return 'offline';
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);

    return diffMinutes <= 3 ? 'online' : 'offline';
  };

  const getDeviceName = (device: Device) => {
    return device.system_info?.hostname || 
           device.system_info?.device_tree?.model || 
           device.hostname ||
           device.serial_number;
  };

  const getStatusIcon = (device: Device) => {
    const status = getDeviceStatus(device);
    
    if (!device.last_seen) {
      return <AlertTriangle className="w-4 h-4 text-gray-500" />;
    }
    
    if (status === 'online') {
      return <CheckCircle className="w-4 h-4 text-green-500" />;
    }
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffDays = (now.getTime() - lastSeen.getTime()) / (1000 * 60 * 60 * 24);
    
    if (diffDays <= 1) {
      return <Clock className="w-4 h-4 text-yellow-500" />;
    }
    
    return <XCircle className="w-4 h-4 text-red-500" />;
  };

  const getStatusLabel = (device: Device) => {
    const status = getDeviceStatus(device);
    
    if (!device.last_seen) return 'Never seen';
    if (status === 'online') return 'Online';
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);
    const diffHours = diffMinutes / 60;
    const diffDays = diffHours / 24;
    
    if (diffDays > 1) return `${Math.floor(diffDays)}d ago`;
    if (diffHours > 1) return `${Math.floor(diffHours)}h ago`;
    return `${Math.floor(diffMinutes)}m ago`;
  };

  if (loading || !release) {
    return (
      <PrivateLayout id="distributions">
        <div className="flex items-center justify-center h-32">
          <div className="text-gray-500 text-sm">Loading...</div>
        </div>
      </PrivateLayout>
    );
  }

  if (error) {
    return (
      <PrivateLayout id="distributions">
        <div className="flex items-center justify-center h-32">
          <div className="text-red-500 text-sm">Error: {error}</div>
        </div>
      </PrivateLayout>
    );
  }

  return (
    <PrivateLayout id="distributions">
      <div className="space-y-6">
        {/* Header with Back Button */}
        <div className="flex items-center space-x-4">
          <button
            onClick={() => distribution ? router.push(`/distributions/${distribution.id}`) : router.push('/distributions')}
            className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="text-sm font-medium">
              {distribution ? `Back to ${distribution.name}` : 'Back to Distributions'}
            </span>
          </button>
        </div>

        {/* Release Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center space-x-4">
            <div className="p-3 bg-green-100 text-green-700 rounded-lg">
              <Tag className="w-6 h-6" />
            </div>
            <div className="flex-1">
              <div className="flex items-center space-x-3">
                <h1 className="text-2xl font-bold text-gray-900">Release {release.version}</h1>
                <span className="text-sm text-gray-500">#{release.id}</span>
                {release.draft && (
                  <span className="px-2 py-1 text-xs font-medium bg-yellow-100 text-yellow-800 rounded-full">
                    Draft
                  </span>
                )}
                {release.yanked && (
                  <span className="px-2 py-1 text-xs font-medium bg-red-100 text-red-800 rounded-full">
                    Yanked
                  </span>
                )}
              </div>
              <div className="flex items-center space-x-6 mt-2 text-sm text-gray-600">
                <div className="flex items-center space-x-1">
                  <Calendar className="w-4 h-4" />
                  <span>Created {formatDate(release.created_at)}</span>
                </div>
                {distribution && (
                  <span className="font-medium">{distribution.name}</span>
                )}
                {release.size && (
                  <span>{formatFileSize(release.size)}</span>
                )}
                {release.download_count !== undefined && (
                  <div className="flex items-center space-x-1">
                    <Download className="w-4 h-4" />
                    <span>{release.download_count} downloads</span>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Tab Navigation */}
        <div className="border-b border-gray-200">
          <nav className="-mb-px flex space-x-8">
            <button
              onClick={() => setActiveTab('packages')}
              className={`py-2 px-1 border-b-2 font-medium text-sm ${
                activeTab === 'packages'
                  ? 'border-blue-500 text-blue-600'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
              }`}
            >
              <div className="flex items-center space-x-2">
                <Box className="w-4 h-4" />
                <span>Packages</span>
                {packages.length > 0 && (
                  <span className="ml-1 bg-gray-100 text-gray-600 py-1 px-2 rounded-full text-xs">
                    {packages.length}
                  </span>
                )}
              </div>
            </button>
            <button
              onClick={() => setActiveTab('devices')}
              className={`py-2 px-1 border-b-2 font-medium text-sm ${
                activeTab === 'devices'
                  ? 'border-blue-500 text-blue-600'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
              }`}
            >
              <div className="flex items-center space-x-2">
                <Users className="w-4 h-4" />
                <span>Devices</span>
                {devices.length > 0 && (
                  <span className="ml-1 bg-gray-100 text-gray-600 py-1 px-2 rounded-full text-xs">
                    {devices.length}
                  </span>
                )}
              </div>
            </button>
          </nav>
        </div>

        {/* Tab Content */}
        {activeTab === 'packages' && (
          <div className="space-y-6">
            <div className="bg-white rounded border border-gray-200 overflow-hidden">
              {packagesLoading ? (
                <div className="p-6 text-center">
                  <div className="text-gray-500 text-sm">Loading packages...</div>
                </div>
              ) : packages.length === 0 ? (
                <div className="p-6 text-center">
                  <Box className="w-8 h-8 text-gray-400 mx-auto mb-2" />
                  <p className="text-sm text-gray-500">No packages found for this release</p>
                </div>
              ) : (
                <div className="divide-y divide-gray-200">
                  {packages.map((pkg) => (
                    <div key={pkg.id} className="p-4">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center space-x-3">
                          <div className="p-2 bg-blue-100 text-blue-700 rounded">
                            <Package className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="flex items-center space-x-2">
                              <h4 className="font-medium text-gray-900">{pkg.name}</h4>
                              <span className="text-xs text-gray-500">v{pkg.version}</span>
                            </div>
                            {pkg.description && (
                              <p className="mt-1 text-sm text-gray-600">{pkg.description}</p>
                            )}
                            <div className="flex items-center space-x-4 mt-1 text-xs text-gray-500">
                              {pkg.size && (
                                <span>{formatFileSize(pkg.size)}</span>
                              )}
                              {pkg.checksum && (
                                <span className="font-mono">{pkg.checksum.substring(0, 16)}...</span>
                              )}
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
        )}

        {activeTab === 'devices' && (
          <div className="space-y-6">
            <div className="bg-white rounded border border-gray-200 overflow-hidden">
              {devicesLoading ? (
                <div className="p-6 text-center">
                  <div className="text-gray-500 text-sm">Loading devices...</div>
                </div>
              ) : devices.length === 0 ? (
                <div className="p-6 text-center">
                  <Users className="w-8 h-8 text-gray-400 mx-auto mb-2" />
                  <p className="text-sm text-gray-500">No devices found using this release</p>
                </div>
              ) : (
                <div className="divide-y divide-gray-200">
                  {devices.map((device) => (
                    <div 
                      key={device.id} 
                      className="p-4 hover:bg-gray-50 cursor-pointer transition-colors"
                      onClick={() => router.push(`/devices/${device.serial_number}`)}
                    >
                      <div className="flex items-center justify-between">
                        <div className="flex items-center space-x-3">
                          <div className="p-2 bg-gray-100 text-gray-700 rounded">
                            <Cpu className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="flex items-center space-x-2">
                              <h4 className="font-medium text-gray-900">{getDeviceName(device)}</h4>
                              <span className="text-xs text-gray-500 font-mono">{device.serial_number}</span>
                            </div>
                            <div className="flex items-center space-x-4 mt-1 text-xs text-gray-500">
                              <div className="flex items-center space-x-1">
                                {getStatusIcon(device)}
                                <span>{getStatusLabel(device)}</span>
                              </div>
                              {device.target_release_id && device.target_release_id !== device.release_id && (
                                <span className="px-1.5 py-0.5 bg-orange-100 text-orange-800 rounded text-xs font-medium">
                                  Update Pending
                                </span>
                              )}
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
        )}
      </div>
    </PrivateLayout>
  );
};

export default ReleaseDetailPage;