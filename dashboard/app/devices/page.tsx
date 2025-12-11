'use client';

import React, { useState, useEffect } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import {
  Cpu,
  Search,
  GitBranch,
  Tag,
  Loader2,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";
import NetworkQualityIndicator from '@/app/components/NetworkQualityIndicator';

const Tooltip = ({ children, content }: { children: React.ReactNode, content: string }) => {
  const [isVisible, setIsVisible] = useState(false);
  const [position, setPosition] = useState<'top' | 'right'>('top');
  const containerRef = React.useRef<HTMLDivElement>(null);

  const handleMouseEnter = () => {
    setIsVisible(true);
    if (containerRef.current) {
      const rect = containerRef.current.getBoundingClientRect();
      
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
      <div className="col-span-2">
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
        <div className="flex gap-1">
          <div className="h-5 bg-gray-300 rounded-full w-16"></div>
          <div className="h-5 bg-gray-300 rounded-full w-16"></div>
        </div>
      </div>

      <div className="col-span-2">
        <div className="flex items-center space-x-2">
          <div className="w-4 h-3 bg-gray-300 rounded-sm flex-shrink-0"></div>
          <div className="h-4 bg-gray-300 rounded w-20"></div>
        </div>
      </div>

      <div className="col-span-1">
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

interface DeviceNetwork {
  network_score?: number
  download_speed_mbps?: number
  upload_speed_mbps?: number
  source?: string
  updated_at?: string
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
  network: DeviceNetwork | null
  labels: Record<string, string>
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
  const searchParams = useSearchParams();
  const { callAPI } = useSmithAPI();
  const [filteredDevices, setFilteredDevices] = useState<Device[]>([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [debouncedSearchTerm, setDebouncedSearchTerm] = useState('');
  const [showOutdatedOnly, setShowOutdatedOnly] = useState(false);
  const [labelFilters, setLabelFilters] = useState<string[]>([]);
  const [onlineStatusFilter, setOnlineStatusFilter] = useState<'all' | 'online' | 'offline'>('all');
  const [isSearching, setIsSearching] = useState(false);

  // Build query params for API call
  const buildQueryParams = () => {
    const params = new URLSearchParams();
    if (labelFilters.length > 0) {
      labelFilters.forEach((filter) => {
        params.append('labels', filter);
      });
    }
    if (onlineStatusFilter === 'online') {
      params.set('online', 'true');
    } else if (onlineStatusFilter === 'offline') {
      params.set('online', 'false');
    }
    return params.toString();
  };

  const queryString = buildQueryParams();
  const endpoint = queryString ? `/devices?${queryString}` : '/devices';

  const { data: devices = [], isLoading: loading } = useQuery({
    queryKey: ['devices', labelFilters, onlineStatusFilter],
    queryFn: () => callAPI<Device[]>('GET', endpoint),
    refetchInterval: 5000,
    select: (data) => data || [],
  });

  // Debounce search term
  useEffect(() => {
    setIsSearching(true);
    const timer = setTimeout(() => {
      setDebouncedSearchTerm(searchTerm);
      setIsSearching(false);
    }, 300);

    return () => {
      clearTimeout(timer);
      setIsSearching(false);
    };
  }, [searchTerm]);

  // Sync URL parameters with component state
  useEffect(() => {
    const outdated = searchParams.get('outdated');
    const online = searchParams.get('online');
    const labelsParam = searchParams.get('labels');

    if (outdated === 'true') {
      setShowOutdatedOnly(true);
    }

    if (online) {
      setOnlineStatusFilter(online as 'all' | 'online' | 'offline');
    }

    if (labelsParam) {
      const parsedLabels = labelsParam.split(',');
      setLabelFilters(parsedLabels);
    }
  }, [searchParams]);

  // Update URL when filters change
  const updateURL = (params: Record<string, string | null>) => {
    const current = new URLSearchParams(Array.from(searchParams.entries()));

    Object.entries(params).forEach(([key, value]) => {
      if (value === null || value === '') {
        current.delete(key);
      } else {
        current.set(key, value);
      }
    });

    const search = current.toString();
    const query = search ? `?${search}` : '';
    router.replace(`/devices${query}`);
  };


  // Filter and sort devices - client-side filtering for search and outdated, sorting
  useEffect(() => {
    // Backend handles: online status, labels
    // Client-side handles: search term, outdated filter, sorting
    let filtered = devices.filter(device => device.has_token);

    // Filter to show only outdated devices if toggle is on
    if (showOutdatedOnly) {
      filtered = filtered.filter(device => hasUpdatePending(device));
    }

    // Client-side search filter
    if (debouncedSearchTerm) {
      filtered = filtered.filter(device =>
        device.serial_number?.toLowerCase().includes(debouncedSearchTerm.toLowerCase()) ||
        device.system_info?.hostname?.toLowerCase().includes(debouncedSearchTerm.toLowerCase()) ||
        device.system_info?.device_tree?.model?.toLowerCase().includes(debouncedSearchTerm.toLowerCase())
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
  }, [devices, debouncedSearchTerm, showOutdatedOnly]);

  const getDeviceStatus = (device: Device) => {
    if (!device.last_seen) return 'offline';

    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);

    return diffMinutes <= 3 ? 'online' : 'offline';
  };

  const getDeviceName = (device: Device) => device.serial_number;

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
    const status = getDeviceStatus(device);
    const networkScore = device.network?.network_score;
    const downloadSpeed = device.network?.download_speed_mbps;
    const uploadSpeed = device.network?.upload_speed_mbps;
    const lastSeenText = device.last_seen ? formatTimeAgo(new Date(device.last_seen)) + ' ago' : 'Never';

    if (status === 'offline') {
      return `Offline\nLast seen: ${lastSeenText}`;
    }

    if (!networkScore) {
      return `Online\nLast seen: ${lastSeenText}`;
    }

    const qualityText = networkScore >= 4 ? 'Excellent' : networkScore === 3 ? 'Good' : 'Poor';
    const downloadText = downloadSpeed ? `↓ ${downloadSpeed.toFixed(1)} Mbps` : '';
    const uploadText = uploadSpeed ? `↑ ${uploadSpeed.toFixed(1)} Mbps` : '';
    const speedText = downloadText || uploadText ? ` (${[downloadText, uploadText].filter(Boolean).join(' / ')})` : '';
    const lastTested = device.network?.updated_at ? formatTimeAgo(new Date(device.network.updated_at)) + ' ago' : 'never';

    return `Online - ${qualityText} Network (${networkScore}/5)${speedText}\nLast tested: ${lastTested}\nLast seen: ${lastSeenText}`;
  };

  const hasUpdatePending = (device: Device) => {
    return device.release_id && device.target_release_id && device.release_id !== device.target_release_id;
  };

  const handleOutdatedToggle = () => {
    const newValue = !showOutdatedOnly;
    setShowOutdatedOnly(newValue);
    updateURL({ outdated: newValue ? 'true' : null });
  };

  const addLabelFilter = (labelInput: string) => {
    const parts = labelInput.split('=');
    if (parts.length === 2) {
      const [key, value] = parts;
      const newFilters = [ ...labelFilters, `${key.trim()}=${value.trim()}` ];
      setLabelFilters(newFilters);

      // Update URL
      const labelsString = newFilters
        .join(',');
      updateURL({ labels: labelsString });
    }
  };

  const removeLabelFilter = (labelFilter: string) => {
    const newFilters = structuredClone(labelFilters).filter((currentLabelFilter) => currentLabelFilter != labelFilter);
    setLabelFilters(newFilters);

    // Update URL
    const labelsString = newFilters.length > 0
      ? newFilters.join(',')
      : null;
    updateURL({ labels: labelsString });
  };

  const handleOnlineStatusChange = (status: 'all' | 'online' | 'offline') => {
    setOnlineStatusFilter(status);
    updateURL({ online: status === 'all' ? null : status });
  };


  const formatTimeAgo = (date: Date) => {
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d`;
    if (hours > 0) return `${hours}h`;
    return `${minutes}m`;
  };

  // Calculate counts for display
  const authorizedDevices = devices.filter(device => device.has_token);
  const outdatedDevices = authorizedDevices.filter(device => hasUpdatePending(device));

  return (
    <PrivateLayout id="devices">
      <div className="space-y-6">
        {/* Search and Filters */}
        <div className="flex flex-col space-y-3">
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
            <div className="flex flex-wrap items-center gap-3">
              <div className="relative">
                <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
                <input
                  type="text"
                  placeholder="Search devices..."
                  value={searchTerm}
                  onChange={(e) => setSearchTerm(e.target.value)}
                  className="pl-10 pr-10 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
                />
                {isSearching && (
                  <Loader2 className="absolute right-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4 animate-spin" />
                )}
              </div>

              {/* Online Status Filter */}
              <div className="flex space-x-1">
                <button
                  onClick={() => handleOnlineStatusChange('all')}
                  className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
                    onlineStatusFilter === 'all'
                      ? 'bg-blue-600 text-white'
                      : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
                  }`}
                >
                  All
                </button>
                <button
                  onClick={() => handleOnlineStatusChange('online')}
                  className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
                    onlineStatusFilter === 'online'
                      ? 'bg-green-600 text-white'
                      : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
                  }`}
                >
                  Online
                </button>
                <button
                  onClick={() => handleOnlineStatusChange('offline')}
                  className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
                    onlineStatusFilter === 'offline'
                      ? 'bg-gray-600 text-white'
                      : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
                  }`}
                >
                  Offline
                </button>
              </div>

              {/* Outdated Filter */}
              {outdatedDevices.length > 0 && (
                <button
                  onClick={handleOutdatedToggle}
                  className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
                    showOutdatedOnly
                      ? 'bg-orange-600 text-white'
                      : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
                  }`}
                >
                  Outdated ({outdatedDevices.length})
                </button>
              )}

              {/* Label Filter Input */}
              <input
                type="text"
                placeholder="Filter by label(e.g., department=slug)"
                className="w-64 px-3 py-2 text-sm border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 placeholder-gray-400"
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    const input = e.currentTarget;
                    addLabelFilter(input.value);
                    input.value = '';
                  }
                }}
              />

              {/* Active Label Filters - inline */}
              {labelFilters.length > 0 && (
                <>
                  {labelFilters.map((filter) => (
                    <div
                      key={filter}
                      className="flex items-center space-x-1 px-2 py-1 text-sm bg-gray-100 text-gray-700 rounded border border-gray-200"
                    >
                      <code className="font-mono text-xs">{filter}</code>
                      <button
                        onClick={() => removeLabelFilter(filter)}
                        className="text-gray-600 hover:text-gray-800 font-bold cursor-pointer"
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </>
              )}
            </div>

            <div className="flex items-center space-x-3">
              <span className="text-sm text-gray-500">
                {loading ? 'Loading...' : `${filteredDevices.length} device${filteredDevices.length !== 1 ? 's' : ''}`}
              </span>
            </div>
          </div>
        </div>

        {/* Device List */}
        <div className="border border-gray-200 rounded-lg overflow-hidden bg-white">
          <div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
            <div className="grid grid-cols-8 gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide">
              <div className="col-span-2">Device</div>
              <div className="col-span-2">Labels</div>
              <div className="col-span-2">Location</div>
              <div className="col-span-1">OS</div>
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
                  <div className="col-span-2">
                    <div className="flex items-center space-x-3">
                      <Cpu className="w-4 h-4 text-gray-400 flex-shrink-0" />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center space-x-2">
                          <Tooltip content={getStatusTooltip(device)}>
                            <div className="flex-shrink-0 cursor-help">
                              <NetworkQualityIndicator
                                isOnline={getDeviceStatus(device) === 'online'}
                                networkScore={device.network?.network_score}
                              />
                            </div>
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
                      </div>
                    </div>
                  </div>

                  <div className="col-span-2">
                    {device.labels && Object.keys(device.labels).length > 0 ? (
                      <div className="flex flex-wrap gap-1">
                        {Object.entries(device.labels).map(([key, value]) => {
                          const filter = `${key}=${value}`
                          const isFiltered = labelFilters.includes(filter);
                          return (
                            <code
                              key={key}
                              onClick={(e) => {
                                e.stopPropagation();
                                if (isFiltered) {
                                  removeLabelFilter(filter);
                                } else {
                                  const newFilters = [...labelFilters, filter];
                                  setLabelFilters(newFilters);
                                  const labelsString = newFilters
                                    .join(',');
                                  updateURL({ labels: labelsString });
                                }
                              }}
                              className={`px-1.5 py-0.5 text-xs font-mono rounded border cursor-pointer transition-colors ${
                                isFiltered
                                  ? 'bg-blue-100 text-blue-800 border-blue-300'
                                  : 'bg-gray-100 text-gray-700 border-gray-200 hover:bg-gray-200'
                              }`}
                            >
                              {key}={value}
                            </code>
                          );
                        })}
                      </div>
                    ) : (
                      <span className="text-xs text-gray-400">-</span>
                    )}
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

                  <div className="col-span-1 text-sm text-gray-600">
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
