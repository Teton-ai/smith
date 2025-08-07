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
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

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

interface Modem {
  id: number;
  imei: string;
  on_dongle: boolean;
  network_provider?: string;
  // Add other modem fields as needed
}

const DeviceDetailPage = () => {
  const params = useParams();
  const router = useRouter();
  const { callAPI } = useSmithAPI();
  const [device, setDevice] = useState<Device | null>(null);
  const [modem, setModem] = useState<Modem | null>(null);
  const [loading, setLoading] = useState(true);

  const serial = params.serial as string;

  useEffect(() => {
    const fetchDevice = async () => {
      setLoading(true);
      try {
        const deviceData = await callAPI<Device>('GET', `/devices/${serial}`);
        if (deviceData) {
          setDevice(deviceData);
          
          // Fetch modem data if device has a modem
          if (deviceData.modem_id) {
            const modemData = await callAPI<Modem>('GET', `/modems/${deviceData.modem_id}`);
            if (modemData) {
              setModem(modemData);
            }
          }
        }
      } finally {
        setLoading(false);
      }
    };
    fetchDevice();
  }, [callAPI, serial]);

  const getDeviceStatus = () => {
    if (!device || !device.last_seen) return 'never-seen';
    
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
        return { color: 'text-green-600', icon: <CheckCircle className="w-5 h-5 text-green-500" />, label: 'Online' };
      case 'stuck-update':
        return { color: 'text-purple-600', icon: <Clock className="w-5 h-5 text-purple-500" />, label: 'Update Failed' };
      case 'recently-offline':
        return { color: 'text-yellow-600', icon: <Clock className="w-5 h-5 text-yellow-500" />, label: 'Recently Offline' };
      case 'offline-week':
        return { color: 'text-orange-600', icon: <XCircle className="w-5 h-5 text-orange-500" />, label: 'Offline (1 week)' };
      case 'offline-month':
        return { color: 'text-red-600', icon: <XCircle className="w-5 h-5 text-red-500" />, label: 'Offline (1 month)' };
      case 'never-seen':
        return { color: 'text-gray-600', icon: <XCircle className="w-5 h-5 text-gray-500" />, label: 'Never Connected' };
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
    
    const status = getDeviceStatus();
    const lastSeenText = device.last_seen ? formatTimeAgo(device.last_seen) : 'Never';
    
    switch (status) {
      case 'online':
        return `Online - Last seen: ${lastSeenText}`;
      case 'stuck-update':
        return `Update Failed - Release ${device.release_id} → ${device.target_release_id}`;
      case 'recently-offline':
        return `Recently Offline - Last seen: ${lastSeenText}`;
      case 'offline-week':
        return `Offline (1 week) - Last seen: ${lastSeenText}`;
      case 'offline-month':
        return `Offline (1 month) - Last seen: ${lastSeenText}`;
      case 'never-seen':
        return 'Never Connected - Device has not come online';
      default:
        return `Status: ${status} - Last seen: ${lastSeenText}`;
    }
  };

  const getStatusDot = (status: string) => {
    switch (status) {
      case 'online':
        return 'bg-green-500 animate-pulse';
      case 'stuck-update':
        return 'bg-purple-500 animate-pulse';
      case 'recently-offline':
        return 'bg-yellow-500';
      case 'offline-week':
        return 'bg-orange-500';
      case 'offline-month':
      case 'never-seen':
        return 'bg-red-500';
      default:
        return 'bg-gray-500';
    }
  };

  const getPrimaryConnectionType = () => {
    if (!device) return null;
    
    // If device has a modem, prioritize cellular
    if (device.modem_id && modem) {
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
        return `Cellular Connection${modem?.network_provider ? ` - ${modem.network_provider}` : ''}${modem ? `\nIMEI: ${modem.imei}` : ''}${modem?.on_dongle ? '\nExternal Dongle' : '\nBuilt-in Modem'}`;
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
          <div className="flex items-start justify-between">
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
                  <h1 className="text-2xl font-bold text-gray-900">{getDeviceName()}</h1>
                </div>
                <p className="text-gray-600 mt-1">{getDeviceModel()}</p>
                <p className="text-sm text-gray-500 font-mono">{device.serial_number}</p>
              </div>
            </div>
            <div className="text-right">
              <div className={`text-sm font-medium ${statusInfo.color}`}>
                {statusInfo.label}
              </div>
              <div className="text-sm text-gray-700">
                {device.last_seen ? `Last seen ${formatTimeAgo(device.last_seen)}` : 'Never seen'}
              </div>
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
              onClick={() => router.push(`/devices/${serial}/releases`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              Releases
            </button>
            <button
              onClick={() => router.push(`/devices/${serial}/packages`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              Packages
            </button>
            <button
              onClick={() => router.push(`/devices/${serial}/logs`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              Logs
            </button>
          </nav>
        </div>

        {/* Overview Content */}
        <div className="space-y-6">

          {/* Device Information */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-4">Device Information</h3>
              <div className="space-y-3">
                {device.system_info?.smith?.version && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Agent</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.smith.version}</span>
                  </div>
                )}
                {device.system_info?.proc?.version && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Kernel Version</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.proc.version}</span>
                  </div>
                )}
                {device.system_info?.os_release?.pretty_name && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Operating System</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.os_release.pretty_name}</span>
                  </div>
                )}
                {device.system_info?.device_tree?.model && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Model</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.device_tree.model}</span>
                  </div>
                )}
                <div className="flex justify-between">
                  <span className="text-gray-700">Registered</span>
                  <span className="text-sm text-gray-900">{new Date(device.created_on).toLocaleString()}</span>
                </div>
                {device.system_info?.proc?.stat?.btime && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Boot Time</span>
                    <span className="text-sm text-gray-900">{new Date(device.system_info.proc.stat.btime * 1000).toLocaleString()}</span>
                  </div>
                )}
                {device.system_info?.hostname && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Hostname</span>
                    <span className="font-mono text-sm text-gray-900">{device.system_info.hostname}</span>
                  </div>
                )}
                {device.modem_id ? (
                  <>
                    <div className="flex justify-between">
                      <span className="text-gray-700">Modem</span>
                      <span className="font-mono text-sm text-gray-900">
                        {modem ? (
                          <div className="text-right">
                            {modem.network_provider && (
                              <div className="font-semibold text-blue-600">
                                {modem.network_provider}
                              </div>
                            )}
                            <div>IMEI: {modem.imei}</div>
                            <div className="text-xs text-gray-600">
                              {modem.on_dongle ? 'External Dongle' : 'Built-in'}
                            </div>
                          </div>
                        ) : (
                          `ID: ${device.modem_id}`
                        )}
                      </span>
                    </div>
                  </>
                ) : (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Modem</span>
                    <span className="text-sm text-gray-500">None</span>
                  </div>
                )}
                {device.release_id && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Current Release</span>
                    <span className="font-mono text-sm text-gray-900">{device.release_id}</span>
                  </div>
                )}
                {device.target_release_id && device.target_release_id !== device.release_id && (
                  <div className="flex justify-between">
                    <span className="text-gray-700">Target Release</span>
                    <span className="font-mono text-sm text-purple-600">{device.target_release_id}</span>
                  </div>
                )}
              </div>
            </div>

            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-4">Network Interfaces</h3>
              {device.system_info?.network?.interfaces ? (
                <div className="space-y-4">
                  {Object.entries(device.system_info.network.interfaces).map(([name, iface]) => {
                    const connectionStatus = device.system_info?.connection_statuses?.find(
                      conn => conn.device_name === name
                    );
                    const isConnected = connectionStatus?.connection_state === 'connected';
                    const deviceType = connectionStatus?.device_type || 'unknown';
                    
                    return (
                      <div key={name} className="border-l-2 border-gray-200 pl-4">
                        <div className="flex items-center justify-between mb-2">
                          <div className="flex items-center space-x-3">
                            {deviceType === 'wifi' ? (
                              isConnected ? <Wifi className="w-4 h-4 text-green-500" /> : <WifiOff className="w-4 h-4 text-gray-400" />
                            ) : (
                              <Cpu className="w-4 h-4 text-blue-500" />
                            )}
                            <span className="font-mono text-sm font-semibold text-gray-900">{name}</span>
                            <span className={`inline-flex px-2 py-1 text-xs font-medium rounded-full ${
                              isConnected 
                                ? 'bg-green-100 text-green-800' 
                                : 'bg-gray-100 text-gray-800'
                            }`}>
                              {connectionStatus?.connection_state || 'unknown'}
                            </span>
                          </div>
                        </div>
                        <div className="text-xs text-gray-600 space-y-1">
                          <div className="flex justify-between">
                            <span>MAC:</span>
                            <span className="font-mono">{iface.mac_address}</span>
                          </div>
                          {iface.ips.length > 0 && (
                            <div className="flex justify-between">
                              <span>IPs:</span>
                              <div className="text-right">
                                {iface.ips.map((ip, ipIndex) => (
                                  <div key={ipIndex} className="font-mono">{ip}</div>
                                ))}
                              </div>
                            </div>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <p className="text-gray-500 text-sm">No network interface information available</p>
              )}
            </div>
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default DeviceDetailPage;