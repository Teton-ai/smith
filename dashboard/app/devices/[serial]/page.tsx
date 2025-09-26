'use client';

import React, { useState, useEffect } from 'react';
import { useParams, useRouter } from 'next/navigation';
import {
  Cpu,
  ChevronRight,
  Wifi,
  WifiOff,
  CheckCircle,
  XCircle,
  Clock,
  Smartphone,
  Router,
  Signal,
  MapPin,
  Globe,
  GitBranch,
  Tag,
  ArrowLeft,
} from 'lucide-react';
import dynamic from 'next/dynamic';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";
import DeviceHeader from './DeviceHeader';

const LocationMap = dynamic(() => import('./LocationMap'), {
  ssr: false,
  loading: () => <div className="h-64 bg-gray-100 rounded-lg flex items-center justify-center">Loading map...</div>
});

const Tooltip = ({ children, content }: { children: React.ReactNode, content: string }) => {
  const [isVisible, setIsVisible] = React.useState(false);
  const [position, setPosition] = React.useState<'top' | 'right'>('top');
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

interface Device {
  id: number;
  serial_number: string;
  note?: string;
  last_seen: string | null;
  has_token: boolean;
  release_id?: number;
  target_release_id?: number;
  created_on: string;
  approved: boolean;
  modem_id?: number;
  ip_address_id?: number;
  ip_address?: IpAddressInfo;
  modem?: Modem;
  release?: Release;
  target_release?: Release;
  system_info?: {
    hostname?: string;
    device_tree?: {
      model?: string;
      serial_number?: string;
      compatible?: string[];
    };
    os_release?: {
      pretty_name?: string;
      version_id?: string;
    };
    proc?: {
      version?: string;
      stat?: {
        btime?: number;
      };
    };
    smith?: {
      version?: string;
    };
    network?: {
      interfaces?: Record<string, {
        ips: string[];
        mac_address: string;
      }>;
    };
    connection_statuses?: Array<{
      connection_name: string;
      connection_state: string;
      device_name: string;
      device_type: string;
    }>;
  };
}


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

interface Modem {
  id: number;
  imei: string;
  network_provider: string;
  updated_at: string;
  created_at: string;
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

const DeviceDetailPage = () => {
  const params = useParams();
  const router = useRouter();
  const { callAPI } = useSmithAPI();
  const [device, setDevice] = useState<Device | null>(null);
  const [loading, setLoading] = useState(true);

  const serial = params.serial as string;

  useEffect(() => {
    const fetchDevice = async () => {
      setLoading(true);
      try {
        const deviceData = await callAPI<Device>('GET', `/devices/${serial}`);
        if (deviceData) {
          setDevice(deviceData);
          

        }
      } finally {
        setLoading(false);
      }
    };
    fetchDevice();
  }, [callAPI, serial]);

  const getDeviceStatus = () => {
    if (!device || !device.last_seen) return 'offline';
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);
    
    return diffMinutes <= 3 ? 'online' : 'offline';
  };

  const getStatusInfo = (status: string) => {
    switch (status) {
      case 'online':
        return { color: 'text-green-600', icon: <CheckCircle className="w-5 h-5 text-green-500" />, label: 'Online' };
      case 'offline':
        return { color: 'text-red-600', icon: <XCircle className="w-5 h-5 text-red-500" />, label: 'Offline' };
      default:
        return { color: 'text-gray-600', icon: <XCircle className="w-5 h-5 text-gray-500" />, label: 'Unknown' };
    }
  };

  const formatTimeAgo = (date: string) => {
    const now = new Date();
    const past = new Date(date);
    const diff = now.getTime() - past.getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d ago`;
    if (hours > 0) return `${hours}h ago`;
    return `${minutes}m ago`;
  };

  const getStatusTooltip = () => {
    if (!device) return '';
    
    const lastSeenText = device.last_seen ? formatTimeAgo(device.last_seen) : 'Never';
    return `Last seen: ${lastSeenText}`;
  };

  const hasUpdatePending = () => {
    return device && device.release_id && device.target_release_id && device.release_id !== device.target_release_id;
  };

  const getStatusDot = (status: string) => {
    switch (status) {
      case 'online':
        return 'bg-green-500 animate-pulse';
      case 'offline':
        return 'bg-red-500';
      default:
        return 'bg-gray-500';
    }
  };

  const getPrimaryConnectionType = () => {
    if (!device) return null;
    
    // If device has a modem, prioritize cellular
    if (device.modem_id && device.modem) {
      return 'cellular';
    }
    
    // Check for active network connections
    const connectedInterfaces = device.system_info?.connection_statuses?.filter(
      conn => conn.connection_state === 'connected'
    );
    
    if (!connectedInterfaces || connectedInterfaces.length === 0) {
      return null;
    }
    
    // Prioritize: WiFi > Ethernet > Other
    if (connectedInterfaces.some(conn => conn.device_type === 'wifi')) {
      return 'wifi';
    }
    
    if (connectedInterfaces.some(conn => conn.device_type === 'ethernet')) {
      return 'ethernet';
    }
    
    return 'other';
  };

  const getConnectionIcon = (connectionType: string | null) => {
    switch (connectionType) {
      case 'cellular':
        return <Signal className="w-4 h-4 text-blue-600" />;
      case 'wifi':
        return <Wifi className="w-4 h-4 text-green-600" />;
      case 'ethernet':
        return <Router className="w-4 h-4 text-orange-600" />;
      default:
        return null;
    }
  };

  const getConnectionTooltip = (connectionType: string | null) => {
    if (!device) return '';
    
    switch (connectionType) {
      case 'cellular':
        return `Cellular Connection${device.modem?.network_provider ? ` - ${device.modem.network_provider}` : ''}${device.modem ? `\nIMEI: ${device.modem.imei}` : ''}${device.modem?.on_dongle ? '\nExternal Dongle' : '\nBuilt-in Modem'}`;
      case 'wifi': {
        const wifiConnections = device.system_info?.connection_statuses?.filter(
          conn => conn.connection_state === 'connected' && conn.device_type === 'wifi'
        );
        const primaryWifi = wifiConnections?.[0];
        return `WiFi Connection${primaryWifi?.connection_name ? ` - ${primaryWifi.connection_name}` : ''}`;
      }
      case 'ethernet': {
        const ethConnections = device.system_info?.connection_statuses?.filter(
          conn => conn.connection_state === 'connected' && conn.device_type === 'ethernet'
        );
        return `Ethernet Connection${ethConnections ? ` - ${ethConnections.length} interface(s)` : ''}`;
      }
      default:
        return 'No active connection detected';
    }
  };

  const getDeviceName = () => device.serial_number;

  const getDeviceModel = () => {
    if (!device) return '';
    return device.system_info?.device_tree?.model || 'Unknown Device';
  };

  const getFlagUrl = (countryCode: string) => {
    return `https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;
  };

  if (loading) {
    return (
      <PrivateLayout id="devices">
        <div className="space-y-6">
          {/* Breadcrumb Skeleton */}
          <div className="flex items-center space-x-2">
            <div className="h-4 bg-gray-200 rounded w-16 animate-pulse" />
            <ChevronRight className="w-4 h-4 text-gray-300" />
            <div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
          </div>

          {/* Header Skeleton */}
          <div className="bg-white rounded-lg border border-gray-200 p-6">
            <div className="flex items-start justify-between">
              <div className="flex items-center space-x-4">
                <div className="p-3 bg-gray-100 rounded-lg">
                  <div className="w-8 h-8 bg-gray-200 rounded animate-pulse" />
                </div>
                <div>
                  <div className="h-8 bg-gray-200 rounded w-48 animate-pulse mb-2" />
                  <div className="h-4 bg-gray-200 rounded w-32 animate-pulse mb-1" />
                  <div className="h-4 bg-gray-200 rounded w-24 animate-pulse" />
                </div>
              </div>
              <div className="text-right">
                <div className="h-4 bg-gray-200 rounded w-16 animate-pulse mb-1" />
                <div className="h-4 bg-gray-200 rounded w-20 animate-pulse" />
              </div>
            </div>
          </div>
        </div>
      </PrivateLayout>
    );
  }

  if (!device) {
    return (
      <PrivateLayout id="devices">
        <div className="space-y-6">
          <div className="text-center py-12">
            <XCircle className="w-12 h-12 text-gray-400 mx-auto mb-4" />
            <h3 className="text-lg font-medium text-gray-900 mb-2">Device not found</h3>
            <p className="text-gray-500">The device with serial number "{serial}" could not be found.</p>
          </div>
        </div>
      </PrivateLayout>
    );
  }

  const status = getDeviceStatus();
  const statusInfo = getStatusInfo(status);
  const connectionType = getPrimaryConnectionType();

  return (
    <PrivateLayout id="devices">
      <div className="space-y-6">
        {/* Header with Back Button */}
        <div className="flex items-center space-x-4">
          <button
            onClick={() => router.push('/devices')}
            className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="text-sm font-medium">Back to Devices</span>
          </button>
        </div>

        {/* Device Header */}
        <DeviceHeader device={device} serial={serial} />

        {/* Tabs */}
        <div className="border-b border-gray-200">
          <nav className="-mb-px flex space-x-8">
            <button
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
            >
              Overview
            </button>
            <button
              onClick={() => router.push(`/devices/${serial}/commands`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              Commands
            </button>
            <button
              onClick={() => router.push(`/devices/${serial}/about`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              About
            </button>
          </nav>
        </div>

        {/* Overview Content */}
        <div className="space-y-6">

          {/* Device Information */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
            {/* Network Section */}
            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-4">Network Connections</h3>
              {device.system_info?.network?.interfaces ? (
                (() => {
                  const activeConnections = Object.entries(device.system_info.network.interfaces).filter(([name]) => {
                    const connectionStatus = device.system_info?.connection_statuses?.find(
                      conn => conn.device_name === name
                    );
                    return connectionStatus?.connection_state === 'connected';
                  });
                  
                  const inactiveConnections = Object.entries(device.system_info.network.interfaces).filter(([name]) => {
                    const connectionStatus = device.system_info?.connection_statuses?.find(
                      conn => conn.device_name === name
                    );
                    return connectionStatus?.connection_state !== 'connected';
                  });
                  
                  if (activeConnections.length === 0 && inactiveConnections.length === 0) {
                    return (
                      <div className="flex items-center text-gray-500 text-sm">
                        <WifiOff className="w-4 h-4 mr-2" />
                        No network interfaces found
                      </div>
                    );
                  }
                  
                  return (
                    <div className="space-y-3">
                      {/* Active Connections */}
                      {activeConnections.map(([name, iface]) => {
                        const connectionStatus = device.system_info?.connection_statuses?.find(
                          conn => conn.device_name === name
                        );
                        const deviceType = connectionStatus?.device_type || 'unknown';
                        const primaryIP = iface.ips[0];
                        
                        return (
                          <div key={name} className="p-3 border border-green-200 bg-green-50 rounded">
                            <div className="flex items-center justify-between mb-2">
                              <div className="flex items-center space-x-2">
                                {deviceType === 'wifi' ? (
                                  <Wifi className="w-4 h-4 text-green-600" />
                                ) : deviceType === 'ethernet' ? (
                                  <Router className="w-4 h-4 text-blue-600" />
                                ) : (
                                  <Smartphone className="w-4 h-4 text-gray-600" />
                                )}
                                <span className="font-mono text-sm font-medium text-gray-900">{name}</span>
                                <div className="w-2 h-2 rounded-full bg-green-500"></div>
                              </div>
                              <span className="text-xs text-green-600 font-medium">Connected</span>
                            </div>
                            
                            <div className="space-y-2 text-sm">
                              {primaryIP && (
                                <div className="flex justify-between">
                                  <span className="text-gray-600">Primary IP</span>
                                  <span className="font-mono text-gray-900">{primaryIP}</span>
                                </div>
                              )}
                              <div className="flex justify-between">
                                <span className="text-gray-600">MAC Address</span>
                                <span className="font-mono text-gray-900">{iface.mac_address}</span>
                              </div>
                              {iface.ips.length > 1 && (
                                <div className="flex justify-between">
                                  <span className="text-gray-600">Additional IPs</span>
                                  <div className="text-right">
                                    {iface.ips.slice(1).map((ip, index) => (
                                      <div key={index} className="font-mono text-gray-900">{ip}</div>
                                    ))}
                                  </div>
                                </div>
                              )}
                            </div>
                          </div>
                        );
                      })}
                      
                      {/* Show inactive connections if any */}
                      {inactiveConnections.length > 0 && (
                        <details className="mt-3">
                          <summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800">
                            Show inactive connections ({inactiveConnections.length})
                          </summary>
                          <div className="mt-2 space-y-2">
                            {inactiveConnections.map(([name, iface]) => {
                              const connectionStatus = device.system_info?.connection_statuses?.find(
                                conn => conn.device_name === name
                              );
                              const deviceType = connectionStatus?.device_type || 'unknown';
                              const primaryIP = iface.ips[0];
                              
                              return (
                                <div key={name} className="p-3 border border-gray-200 bg-gray-50 rounded">
                                  <div className="flex items-center justify-between mb-2">
                                    <div className="flex items-center space-x-2">
                                      {deviceType === 'wifi' ? (
                                        <WifiOff className="w-4 h-4 text-gray-400" />
                                      ) : deviceType === 'ethernet' ? (
                                        <Router className="w-4 h-4 text-gray-400" />
                                      ) : (
                                        <Smartphone className="w-4 h-4 text-gray-400" />
                                      )}
                                      <span className="font-mono text-sm font-medium text-gray-900">{name}</span>
                                      <div className="w-2 h-2 rounded-full bg-gray-400"></div>
                                    </div>
                                    <span className="text-xs text-gray-500 font-medium">Disconnected</span>
                                  </div>
                                  
                                  <div className="space-y-2 text-sm">
                                    {primaryIP && (
                                      <div className="flex justify-between">
                                        <span className="text-gray-600">Primary IP</span>
                                        <span className="font-mono text-gray-900">{primaryIP}</span>
                                      </div>
                                    )}
                                    <div className="flex justify-between">
                                      <span className="text-gray-600">MAC Address</span>
                                      <span className="font-mono text-gray-900">{iface.mac_address}</span>
                                    </div>
                                    {iface.ips.length > 1 && (
                                      <div className="flex justify-between">
                                        <span className="text-gray-600">Additional IPs</span>
                                        <div className="text-right">
                                          {iface.ips.slice(1).map((ip, index) => (
                                            <div key={index} className="font-mono text-gray-900">{ip}</div>
                                          ))}
                                        </div>
                                      </div>
                                    )}
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                        </details>
                      )}
                    </div>
                  );
                })()
              ) : (
                <p className="text-gray-500 text-sm">No network interface information available</p>
              )}
            </div>

            {/* Location Information Section */}
            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <div className="flex items-center space-x-2 mb-4">
                <MapPin className="w-5 h-5 text-blue-600" />
                <h3 className="text-lg font-semibold text-gray-900">Location Information</h3>
              </div>

              {device.ip_address ? (
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    {/* Map */}
                    <div>
                      <LocationMap 
                        countryCode={device.ip_address.country_code}
                        city={device.ip_address.city}
                        country={device.ip_address.country}
                      />
                    </div>

                    {/* Location Details */}
                    <div className="space-y-4">
                      <div className="flex items-center space-x-3">
                        <Globe className="w-4 h-4 text-gray-500" />
                        <span className="font-mono text-sm text-gray-900">{device.ip_address.ip_address}</span>
                        {device.ip_address.country_code && (
                          <img 
                            src={getFlagUrl(device.ip_address.country_code)} 
                            alt={device.ip_address.country || 'Country flag'} 
                            className="w-6 h-4 rounded-sm border border-gray-200"
                            onError={(e) => {
                              (e.target as HTMLImageElement).style.display = 'none';
                            }}
                          />
                        )}
                      </div>
                      
                      <div className="space-y-3">
                        {device.ip_address.name && (
                          <div className="flex items-center justify-between">
                            <span className="text-gray-600">Location Name</span>
                            <span className="text-gray-900 font-medium">{device.ip_address.name}</span>
                          </div>
                        )}
                        {device.ip_address.country && (
                          <div className="flex items-center justify-between">
                            <span className="text-gray-600">Country</span>
                            <span className="text-gray-900 font-medium">
                              {device.ip_address.country}
                              {device.ip_address.country_code && ` (${device.ip_address.country_code})`}
                            </span>
                          </div>
                        )}
                        {device.ip_address.region && (
                          <div className="flex items-center justify-between">
                            <span className="text-gray-600">Region</span>
                            <span className="text-gray-900 font-medium">{device.ip_address.region}</span>
                          </div>
                        )}
                        {device.ip_address.city && (
                          <div className="flex items-center justify-between">
                            <span className="text-gray-600">City</span>
                            <span className="text-gray-900 font-medium">{device.ip_address.city}</span>
                          </div>
                        )}
                        {device.ip_address.isp && (
                          <div className="flex items-center justify-between">
                            <span className="text-gray-600">Internet Provider</span>
                            <span className="text-gray-900 font-medium">{device.ip_address.isp}</span>
                          </div>
                        )}
                        {device.ip_address.coordinates && (
                          <div className="flex items-center justify-between">
                            <span className="text-gray-600">Coordinates</span>
                            <span className="font-mono text-sm text-gray-900">
                              {device.ip_address.coordinates[0].toFixed(4)}, {device.ip_address.coordinates[1].toFixed(4)}
                            </span>
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                ) : (
                  <div className="flex items-center justify-center py-8">
                    <div className="text-center">
                      <Globe className="w-12 h-12 text-gray-300 mx-auto mb-4" />
                      <p className="text-gray-500">No location information available</p>
                      <p className="text-gray-400 text-sm mt-1">This device has no associated IP address data</p>
                    </div>
                  </div>
                )}
            </div>
          </div>

        </div>
      </div>
    </PrivateLayout>
  );
};

export default DeviceDetailPage;