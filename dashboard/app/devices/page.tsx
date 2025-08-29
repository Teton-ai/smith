'use client';

import React, { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';
import {
  Cpu,
  Search,
  GitBranch,
  Eye,
  EyeOff,
  Tag,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

const Tooltip = ({ children, content }: { children: React.ReactNode, content: string }) => {
  const [isVisible, setIsVisible] = useState(false);
  const [position, setPosition] = useState<'top' | 'right'>('top');
  const containerRef = React.useRef<HTMLDivElement>(null);

  const handleMouseEnter = () => {
    setIsVisible(true);
    if (containerRef.current) {
      const rect = containerRef.current.getBoundingClientRect();
      const viewportWidth = window.innerWidth;
      
      // If tooltip would be cut off on the left side, position it to the right
      if (rect.left < 150) {
        setPosition('right');
      } else {
        setPosition('top');
      }
    }
  };

  return (
    <div 
      ref={containerRef}
      className="relative inline-block"
      onMouseEnter={handleMouseEnter}
      onMouseLeave={() => setIsVisible(false)}
    >
      {children}
      {isVisible && (
        <>
          {position === 'top' ? (
            <div className="absolute bottom-full left-1/2 transform -translate-x-1/2 mb-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50">
              {content}
              <div className="absolute top-full left-1/2 transform -translate-x-1/2 w-0 h-0 border-l-4 border-r-4 border-t-4 border-l-transparent border-r-transparent border-t-gray-800"></div>
            </div>
          ) : (
            <div className="absolute left-full top-1/2 transform -translate-y-1/2 ml-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50">
              {content}
              <div className="absolute right-full top-1/2 transform -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-r-4 border-t-transparent border-b-transparent border-r-gray-800"></div>
            </div>
          )}
        </>
      )}
    </div>
  );
};

const DeviceSkeleton = () => (
  <div className="px-4 py-3 animate-pulse">
    <div className="grid grid-cols-8 gap-4 items-center">
      <div className="col-span-3">
        <div className="flex items-center space-x-3">
          <div className="w-4 h-4 bg-gray-300 rounded flex-shrink-0"></div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center space-x-2">
              <div className="w-2 h-2 bg-gray-300 rounded-full flex-shrink-0"></div>
              <div className="h-4 bg-gray-300 rounded w-32"></div>
            </div>
            <div className="h-3 bg-gray-200 rounded w-24 mt-1"></div>
          </div>
        </div>
      </div>
      
      <div className="col-span-2">
        <div className="flex items-center space-x-2">
          <div className="w-4 h-3 bg-gray-300 rounded-sm flex-shrink-0"></div>
          <div className="h-4 bg-gray-300 rounded w-20"></div>
        </div>
      </div>
      
      <div className="col-span-2">
        <div className="h-4 bg-gray-300 rounded w-20"></div>
      </div>
      
      <div className="col-span-1">
        <div className="flex items-center space-x-1">
          <div className="w-3 h-3 bg-gray-300 rounded flex-shrink-0"></div>
          <div className="h-3 bg-gray-300 rounded w-12"></div>
        </div>
      </div>
    </div>
  </div>
);

const LoadingSkeleton = () => (
  <div className="divide-y divide-gray-200">
    {Array.from({ length: 6 }, (_, i) => (
      <DeviceSkeleton key={i} />
    ))}
  </div>
);

interface SystemInfo {
  device_tree?: {
    model?: string
    serial_number?: string
  }
  hostname?: string
  os_release?: {
    pretty_name?: string
    version_id?: string
  }
  smith?: {
    version?: string
  }
  proc?: {
    version?: string
  }
}

interface Device {
  id: number
  serial_number: string
  note?: string
  last_seen: string | null
  created_on: string
  approved: boolean
  has_token: boolean
  release_id: number | null
  target_release_id: number | null
  system_info: SystemInfo | null
  modem_id: number | null
  ip_address_id: number | null
  ip_address: IpAddressInfo | null
  modem: Modem | null
  release: Release | null
  target_release: Release | null
}

interface IpAddressInfo {
  id: number
  ip_address: string
  name?: string
  continent?: string
  continent_code?: string
  country_code?: string
  country?: string
  region?: string
  city?: string
  isp?: string
  coordinates?: [number, number]
  proxy?: boolean
  hosting?: boolean
  created_at: string
  updated_at: string
}

interface Modem {
  id: number
  imei: string
  network_provider: string
  updated_at: string
  created_at: string
}

interface Release {
  id: number
  distribution_id: number
  distribution_architecture: string
  distribution_name: string
  version: string
  draft: boolean
  yanked: boolean
  created_at: string
}

const DevicesPage = () => {
  const router = useRouter();
  const { callAPI, loading, error } = useSmithAPI();
  const [devices, setDevices] = useState<Device[]>([]);
  const [filteredDevices, setFilteredDevices] = useState<Device[]>([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [showLongOfflineDevices, setShowLongOfflineDevices] = useState(false);

  useEffect(() => {
    const fetchDashboard = async () => {
      const data = await callAPI<Device[]>('GET', '/devices');
      if (data) {
        setDevices(data);
      }
    };
    fetchDashboard();
  }, [callAPI]);


  // Filter and sort devices - show only authorized devices, filter by search, and sort by latest online
  useEffect(() => {
    // First filter to only show authorized devices (has_token = true)
    let filtered = devices.filter(device => device.has_token);

    // Filter out long offline devices unless toggle is on
    if (!showLongOfflineDevices) {
      filtered = filtered.filter(device => !isLongOffline(device));
    }

    if (searchTerm) {
      filtered = filtered.filter(device =>
        device.serial_number?.toLowerCase().includes(searchTerm.toLowerCase()) ||
        device.system_info?.hostname?.toLowerCase().includes(searchTerm.toLowerCase()) ||
        device.system_info?.device_tree?.model?.toLowerCase().includes(searchTerm.toLowerCase())
      );
    }

    // Sort devices by latest online (most recent first)
    filtered = filtered.sort((a, b) => {
      // Online devices first, then sort by last seen time (most recent first)
      const statusA = getDeviceStatus(a);
      const statusB = getDeviceStatus(b);
      
      if (statusA === 'online' && statusB !== 'online') return -1;
      if (statusB === 'online' && statusA !== 'online') return 1;
      
      // If both have same status, sort by most recent last_seen
      // Handle null last_seen values (put them at the end)
      if (!a.last_seen && !b.last_seen) return 0;
      if (!a.last_seen) return 1;
      if (!b.last_seen) return -1;
      
      return new Date(b.last_seen).getTime() - new Date(a.last_seen).getTime();
    });

    setFilteredDevices(filtered);
  }, [devices, searchTerm, showLongOfflineDevices]);

  const getDeviceStatus = (device: Device) => {
    if (!device.last_seen) return 'offline';
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);

    return diffMinutes <= 3 ? 'online' : 'offline';
  };

  const isLongOffline = (device: Device) => {
    if (!device.last_seen) return true; // Never seen = long offline
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffDays = (now.getTime() - lastSeen.getTime()) / (1000 * 60 * 60 * 24);
    
    return diffDays > 30;
  };

  const getDeviceName = (device: Device) => {
    return device.system_info?.hostname || 
           device.system_info?.device_tree?.model || 
           device.serial_number;
  };

  const getOSVersion = (device: Device) => {
    const osRelease = device.system_info?.os_release;
    if (!osRelease) return 'Unknown';
    
    if (osRelease.pretty_name) {
      return osRelease.pretty_name;
    }
    
    if (osRelease.version_id) {
      return `Ubuntu ${osRelease.version_id}`;
    }
    
    return 'Unknown';
  };

  const getReleaseInfo = (device: Device) => {
    if (device.release) {
      return {
        distribution: device.release.distribution_name,
        version: device.release.version
      };
    }
    return null;
  };

  const getIpLocationInfo = (device: Device) => {
    return device.ip_address || null;
  };

  const getFlagUrl = (countryCode: string) => {
    return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
  };

  const getStatusTooltip = (device: Device) => {
    return 'Last seen: ' + (device.last_seen ? formatTimeAgo(new Date(device.last_seen)) + ' ago' : 'Never');
  };

  const hasUpdatePending = (device: Device) => {
    return device.release_id && device.target_release_id && device.release_id !== device.target_release_id;
  };


  const formatTimeAgo = (date) => {
    const now = new Date();
    const diff = now - date;
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d`;
    if (hours > 0) return `${hours}h`;
    return `${minutes}m`;
  };

  // Calculate counts for display
  const authorizedDevices = devices.filter(device => device.has_token);
  const longOfflineDevices = authorizedDevices.filter(device => isLongOffline(device));

  return (
    <PrivateLayout id="devices">
      <div className="space-y-6">
        {/* Search and Device Count */}
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center space-x-4">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
              <input
                type="text"
                placeholder="Search devices..."
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value)}
                className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
              />
            </div>
          </div>

          <div className="mt-4 sm:mt-0 flex items-center space-x-3">
            <span className="text-sm text-gray-500">
              {loading ? 'Loading...' : `${filteredDevices.length} device${filteredDevices.length !== 1 ? 's' : ''} shown`}
            </span>
            {longOfflineDevices.length > 0 && (
              <button
                onClick={() => setShowLongOfflineDevices(!showLongOfflineDevices)}
                className="flex items-center space-x-1 text-blue-600 hover:text-blue-800 text-sm"
              >
                {showLongOfflineDevices ? <EyeOff className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
                <span>
                  {showLongOfflineDevices ? 'Hide' : 'Show'} {longOfflineDevices.length} long-offline
                </span>
              </button>
            )}
          </div>
        </div>

        {/* Device List */}
        <div className="border border-gray-200 rounded-lg overflow-hidden bg-white">
          <div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
            <div className="grid grid-cols-8 gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide">
              <div className="col-span-3">Device</div>
              <div className="col-span-2">Location</div>
              <div className="col-span-2">OS</div>
              <div className="col-span-1">Release</div>
            </div>
          </div>

          {loading ? (
            <LoadingSkeleton />
          ) : (
            <div className="divide-y divide-gray-200">
              {filteredDevices.map((device) => (
              <div 
                key={device.id} 
                className="px-4 py-3 hover:bg-gray-50 cursor-pointer transition-colors"
                onClick={() => router.push(`/devices/${device.serial_number}`)}
              >
                <div className="grid grid-cols-8 gap-4 items-center">
                  <div className="col-span-3">
                    <div className="flex items-center space-x-3">
                      <Cpu className="w-4 h-4 text-gray-400 flex-shrink-0" />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center space-x-2">
                          <Tooltip content={getStatusTooltip(device)}>
                            <div 
                              className={`w-2 h-2 rounded-full flex-shrink-0 cursor-help ${
                                getDeviceStatus(device) === 'online' 
                                  ? 'bg-green-500 animate-pulse' 
                                  : 'bg-red-500'
                              }`}
                            ></div>
                          </Tooltip>
                          <div className="flex items-center space-x-2 min-w-0 flex-1">
                            <div className="text-sm font-medium text-gray-900 truncate">
                              {getDeviceName(device)}
                            </div>
                            {hasUpdatePending(device) && (
                              <Tooltip content={`Update pending: Release ${device.release_id} → ${device.target_release_id}`}>
                                <span className="px-1.5 py-0.5 text-xs font-medium bg-orange-100 text-orange-800 rounded-full cursor-help flex-shrink-0">
                                  Outdated
                                </span>
                              </Tooltip>
                            )}
                          </div>
                        </div>
                        <div className="text-xs text-gray-500 font-mono mt-0.5">
                          {device.serial_number}
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="col-span-2">
                    {(() => {
                      const ipInfo = getIpLocationInfo(device);
                      if (!ipInfo) {
                        return <div className="text-sm text-gray-400">No location data</div>;
                      }
                      
                      const locationParts = [];
                      if (ipInfo.name) locationParts.push(ipInfo.name);
                      if (ipInfo.city) locationParts.push(ipInfo.city);
                      if (ipInfo.country) locationParts.push(ipInfo.country);
                      
                      return (
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
                            {locationParts.join(', ') || 'Unknown location'}
                          </div>
                        </div>
                      );
                    })()}
                  </div>

                  <div className="col-span-2 text-sm text-gray-600">
                    {getOSVersion(device)}
                  </div>

                  <div className="col-span-1">
                    {getReleaseInfo(device) ? (
                      <div className="flex flex-col space-y-1">
                        <div className="flex items-center space-x-1">
                          <GitBranch className="w-3 h-3 text-gray-400" />
                          <span className="text-xs font-mono text-gray-600">
                            {getReleaseInfo(device)!.distribution}
                          </span>
                        </div>
                        <div className="flex items-center space-x-1">
                          <Tag className="w-3 h-3 text-gray-400" />
                          <span className="text-xs font-mono text-gray-600">
                            {getReleaseInfo(device)!.version}
                          </span>
                        </div>
                      </div>
                    ) : (
                      <div className="flex items-center space-x-1">
                        <GitBranch className="w-3 h-3 text-gray-400" />
                        <span className="text-xs text-gray-500">No Release</span>
                      </div>
                    )}
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

export default DevicesPage;
