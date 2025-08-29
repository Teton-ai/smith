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
} from 'lucide-react';
import dynamic from 'next/dynamic';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

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

  const getDeviceName = () => {
    if (!device) return serial;
    return device.system_info?.hostname || 
           device.hostname || 
           device.system_info?.device_tree?.model || 
           device.serial_number;
  };

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
        {/* Breadcrumb Navigation */}
        <div className="flex items-center space-x-2 text-sm text-gray-500">
          <button 
            onClick={() => router.push('/devices')}
            className="hover:text-gray-700 transition-colors cursor-pointer"
          >
            Devices
          </button>
          <ChevronRight className="w-4 h-4" />
          <span className="text-gray-900 font-medium">{serial}</span>
        </div>

        {/* Device Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center space-x-4">
            <div className="p-3 bg-gray-100 rounded-lg">
              <Cpu className="w-8 h-8 text-gray-600" />
            </div>
            <div>
              <div className="flex items-center space-x-3">
                <div className="flex items-center space-x-2">
                  <Tooltip content={getStatusTooltip()}>
                    <div className={`w-3 h-3 rounded-full flex-shrink-0 cursor-help ${getStatusDot(status)}`}></div>
                  </Tooltip>
                  {connectionType && (
                    <Tooltip content={getConnectionTooltip(connectionType)}>
                      <div className="cursor-help">
                        {getConnectionIcon(connectionType)}
                      </div>
                    </Tooltip>
                  )}
                </div>
                <div className="flex items-center space-x-3">
                  <h1 className="text-2xl font-bold text-gray-900">{getDeviceName()}</h1>
                  {hasUpdatePending() && (
                    <Tooltip content={`Update pending: ${device.release?.version || device.release_id} → ${device.target_release?.version || device.target_release_id}`}>
                      <span className="px-2 py-1 text-xs font-medium bg-orange-100 text-orange-800 rounded-full cursor-help">
                        Outdated
                      </span>
                    </Tooltip>
                  )}
                </div>
              </div>
              <p className="text-gray-600 mt-1">{getDeviceModel()}</p>
              <p className="text-sm text-gray-500 font-mono">{device.serial_number}</p>
            </div>
          </div>
        </div>

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
              onClick={() => router.push(`/devices/${serial}/packages`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              Packages
            </button>
          </nav>
        </div>

        {/* Overview Content */}
        <div className="space-y-6">

          {/* Device Information */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-4">System Information</h3>
              <div className="space-y-3">
                {device.system_info?.smith?.version && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Agent</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.smith.version}</span>
                  </div>
                )}
                {device.system_info?.os_release?.pretty_name && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Operating System</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.os_release.pretty_name}</span>
                  </div>
                )}
                {device.system_info?.proc?.version && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Kernel</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.proc.version}</span>
                  </div>
                )}
                {device.system_info?.proc?.stat?.btime && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Boot Time</span>
                    <span className="text-sm text-gray-900">{new Date(device.system_info.proc.stat.btime * 1000).toLocaleString()}</span>
                  </div>
                )}
                {device.release && (
                  <>
                    <div className="flex justify-between">
                      <span className="text-gray-700 flex items-center">
                        <GitBranch className="w-4 h-4 text-gray-400 mr-2" />
                        Distribution
                      </span>
                      <span className="font-mono text-sm text-gray-900">
                        {device.release.distribution_name}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-700 flex items-center">
                        <Tag className="w-4 h-4 text-gray-400 mr-2" />
                        Current Release
                      </span>
                      <span className="font-mono text-sm text-gray-900">
                        {device.release.version}
                      </span>
                    </div>
                  </>
                )}
                {device.target_release && device.target_release_id !== device.release_id && (
                  <div className="flex justify-between">
                    <span className="text-gray-700 flex items-center">
                      <Tag className="w-4 h-4 text-purple-400 mr-2" />
                      Target Release
                    </span>
                    <span className="font-mono text-sm text-gray-900">
                      {device.target_release.version}
                    </span>
                  </div>
                )}
              </div>
            </div>

            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <div className="flex items-center justify-between mb-4">
                <h3 className="text-lg font-semibold text-gray-900">Network</h3>
                {device.system_info?.network?.interfaces && (
                  <span className="text-sm text-gray-600">
                    {Object.entries(device.system_info.network.interfaces).filter(([name]) => {
                      const connectionStatus = device.system_info?.connection_statuses?.find(
                        conn => conn.device_name === name
                      );
                      return connectionStatus?.connection_state === 'connected';
                    }).length} active connections
                  </span>
                )}
              </div>
              {device.system_info?.network?.interfaces ? (
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
                  {Object.entries(device.system_info.network.interfaces).map(([name, iface]) => {
                    const connectionStatus = device.system_info?.connection_statuses?.find(
                      conn => conn.device_name === name
                    );
                    const isConnected = connectionStatus?.connection_state === 'connected';
                    const deviceType = connectionStatus?.device_type || 'unknown';
                    const primaryIP = iface.ips[0];
                    
                    return (
                      <div key={name} className={`p-3 rounded-lg border-2 transition-colors ${
                        isConnected 
                          ? 'border-green-200 bg-green-50' 
                          : 'border-gray-200 bg-gray-50'
                      }`}>
                        <div className="flex items-center space-x-2 mb-2">
                          {deviceType === 'wifi' ? (
                            isConnected ? <Wifi className="w-4 h-4 text-green-600" /> : <WifiOff className="w-4 h-4 text-gray-400" />
                          ) : deviceType === 'ethernet' ? (
                            <Router className="w-4 h-4 text-blue-600" />
                          ) : (
                            <Smartphone className="w-4 h-4 text-gray-600" />
                          )}
                          <span className="font-mono text-sm font-medium text-gray-900">{name}</span>
                          <div className={`w-2 h-2 rounded-full ${
                            isConnected ? 'bg-green-500' : 'bg-gray-400'
                          }`}></div>
                        </div>
                        {primaryIP && (
                          <div className="text-xs font-mono text-gray-700 mb-1">
                            {primaryIP}
                          </div>
                        )}
                        <div className="text-xs text-gray-600">
                          {connectionStatus?.connection_state || 'disconnected'}
                          {iface.ips.length > 1 && (
                            <span className="ml-2 text-gray-500">+{iface.ips.length - 1} more</span>
                          )}
                        </div>
                        <details className="mt-2">
                          <summary className="text-xs text-blue-600 cursor-pointer hover:text-blue-800">
                            Details
                          </summary>
                          <div className="mt-2 pt-2 border-t border-gray-200 space-y-1">
                            <div className="text-xs text-gray-600">
                              <span className="font-medium">MAC:</span> <span className="font-mono">{iface.mac_address}</span>
                            </div>
                            {iface.ips.length > 1 && (
                              <div className="text-xs text-gray-600">
                                <span className="font-medium">All IPs:</span>
                                <div className="ml-2 space-y-1">
                                  {iface.ips.map((ip, ipIndex) => (
                                    <div key={ipIndex} className="font-mono">{ip}</div>
                                  ))}
                                </div>
                              </div>
                            )}
                          </div>
                        </details>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <p className="text-gray-500 text-sm">No network interface information available</p>
              )}
            </div>
          </div>

          {/* Location Information Section */}
          <div className="bg-white rounded-lg border border-gray-200 p-6">
            <div className="flex items-center space-x-2 mb-4">
              <MapPin className="w-5 h-5 text-blue-600" />
              <h3 className="text-lg font-semibold text-gray-900">Location Information</h3>
            </div>

            {device.ip_address ? (
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
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
                      <div className="text-xs text-gray-500 pt-2 border-t border-gray-200">
                        Last updated: {new Date(device.ip_address.updated_at).toLocaleString()}
                      </div>
                    </div>
                  </div>

                  {/* Map */}
                  <div className="">
                    <LocationMap 
                      countryCode={device.ip_address.country_code}
                      city={device.ip_address.city}
                      country={device.ip_address.country}
                    />
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
    </PrivateLayout>
  );
};

export default DeviceDetailPage;