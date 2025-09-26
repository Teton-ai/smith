'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import {
  Cpu,
  Wifi,
  WifiOff,
  Smartphone,
  Router,
  Signal,
  Tag,
  GitBranch,
  Terminal,
  Copy,
  Check,
} from 'lucide-react';

const Tooltip = ({ children, content }: { children: React.ReactNode, content: string }) => {
  const [isVisible, setIsVisible] = React.useState(false);
  const [position, setPosition] = React.useState<'top' | 'right' | 'left'>('top');
  const containerRef = React.useRef<HTMLDivElement>(null);

  const handleMouseEnter = () => {
    setIsVisible(true);
    if (containerRef.current) {
      const rect = containerRef.current.getBoundingClientRect();
      const viewportWidth = window.innerWidth;

      // Estimate tooltip width (adjust based on content length)
      const estimatedTooltipWidth = content.length * 8 + 32; // rough estimate

      // If tooltip would be cut off on the right side, position it to the left
      if (rect.right + estimatedTooltipWidth > viewportWidth - 20) { // 20px buffer
        setPosition('left');
      } else if (rect.left < 150) {
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
            <div className="absolute bottom-full left-1/2 transform -translate-x-1/2 mb-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50 pointer-events-none">
              {content}
              <div className="absolute top-full left-1/2 transform -translate-x-1/2 w-0 h-0 border-l-4 border-r-4 border-t-4 border-l-transparent border-r-transparent border-t-gray-800"></div>
            </div>
          ) : position === 'right' ? (
            <div className="absolute left-full top-1/2 transform -translate-y-1/2 ml-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50 pointer-events-none">
              {content}
              <div className="absolute right-full top-1/2 transform -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-r-4 border-t-transparent border-b-transparent border-r-gray-800"></div>
            </div>
          ) : (
            <div className="absolute right-full top-1/2 transform -translate-y-1/2 mr-2 px-2 py-1 text-xs text-white bg-gray-800 rounded shadow-lg whitespace-nowrap z-50 pointer-events-none">
              {content}
              <div className="absolute left-full top-1/2 transform -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-l-4 border-t-transparent border-b-transparent border-l-gray-800"></div>
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
  modem?: {
    id: number;
    imei: string;
    network_provider: string;
    updated_at: string;
    created_at: string;
    on_dongle?: boolean;
  };
  release?: {
    id: number;
    distribution_id: number;
    distribution_architecture: string;
    distribution_name: string;
    version: string;
    draft: boolean;
    yanked: boolean;
    created_at: string;
  };
  target_release?: {
    id: number;
    distribution_id: number;
    distribution_architecture: string;
    distribution_name: string;
    version: string;
    draft: boolean;
    yanked: boolean;
    created_at: string;
  };
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

interface DeviceHeaderProps {
  device: Device;
  serial: string;
}

const DeviceHeader: React.FC<DeviceHeaderProps> = ({ device, serial }) => {
  const [sshCopied, setSshCopied] = React.useState(false);

  const handleSshTunnel = async () => {
    const command = `sm tunnel ${device?.serial_number || serial}`;
    try {
      await navigator.clipboard.writeText(command);
      setSshCopied(true);
      setTimeout(() => setSshCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy to clipboard:', err);
    }
  };
  if (!device) {
    return (
      <div className="bg-white rounded-lg border border-gray-200 p-4">
        <div className="flex items-center space-x-3">
          <div className="p-2 bg-gray-100 text-gray-600 rounded">
            <Cpu className="w-5 h-5" />
          </div>
          <div className="flex-1">
            <div className="flex items-center space-x-3">
              <h1 className="text-xl font-bold text-gray-900">{serial}</h1>
            </div>
            <div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
              <span>Loading...</span>
            </div>
          </div>
          <div className="flex-shrink-0">
            <Tooltip content={sshCopied ? "Copied to clipboard!" : `Copy SSH tunnel command: sm tunnel ${serial}`}>
              <button
                onClick={handleSshTunnel}
                className={`flex items-center space-x-2 px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                  sshCopied
                    ? 'bg-green-100 text-green-800 border border-green-200'
                    : 'bg-blue-50 text-blue-700 border border-blue-200 hover:bg-blue-100'
                }`}
              >
                {sshCopied ? (
                  <>
                    <Check className="w-4 h-4" />
                    <span>Copied!</span>
                  </>
                ) : (
                  <>
                    <Terminal className="w-4 h-4" />
                    <Copy className="w-3 h-3" />
                    <span>SSH</span>
                  </>
                )}
              </button>
            </Tooltip>
          </div>
        </div>
      </div>
    );
  }

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

  const getDeviceStatus = () => {
    if (!device || !device.last_seen) return 'offline';
    
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now.getTime() - lastSeen.getTime()) / (1000 * 60);
    
    return diffMinutes <= 3 ? 'online' : 'offline';
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

  const getDeviceName = () => device?.serial_number || serial;

  const status = getDeviceStatus();
  const connectionType = getPrimaryConnectionType();

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-4">
      <div className="flex items-center space-x-3">
        <div className="p-2 bg-gray-100 text-gray-600 rounded">
          <Cpu className="w-5 h-5" />
        </div>
        <div className="flex-1">
          <div className="flex items-center space-x-3">
            <h1 className="text-xl font-bold text-gray-900">{getDeviceName()}</h1>
            <Tooltip content={getStatusTooltip()}>
              <div className={`w-2 h-2 rounded-full flex-shrink-0 cursor-help ${getStatusDot(status)}`}></div>
            </Tooltip>
            {hasUpdatePending() && (
              <Tooltip content={`Update pending: ${device.release?.version || device.release_id} → ${device.target_release?.version || device.target_release_id}`}>
                <span className="px-2 py-1 text-xs font-medium bg-orange-100 text-orange-800 rounded-full cursor-help">
                  Outdated
                </span>
              </Tooltip>
            )}
          </div>
          <div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
            {device.release && (
              <div className="flex items-center space-x-2">
                <div className="flex items-center space-x-1">
                  <GitBranch className="w-4 h-4" />
                  <span className="font-medium">{device.release.distribution_name}</span>
                </div>
                <div className="flex items-center space-x-1">
                  <Tag className="w-4 h-4" />
                  <span>v{device.release.version}</span>
                </div>
              </div>
            )}
            {connectionType && (
              <Tooltip content={getConnectionTooltip(connectionType)}>
                <div className="flex items-center space-x-1 cursor-help">
                  {getConnectionIcon(connectionType)}
                  <span className="capitalize">{connectionType}</span>
                </div>
              </Tooltip>
            )}
          </div>
        </div>
        <div className="flex-shrink-0">
          <Tooltip content={sshCopied ? "Copied to clipboard!" : `Copy SSH tunnel command: sm tunnel ${device?.serial_number || serial}`}>
            <button
              onClick={handleSshTunnel}
              className={`flex items-center space-x-2 px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                sshCopied
                  ? 'bg-green-100 text-green-800 border border-green-200'
                  : 'bg-blue-50 text-blue-700 border border-blue-200 hover:bg-blue-100'
              }`}
            >
              {sshCopied ? (
                <>
                  <Check className="w-4 h-4" />
                  <span>Copied!</span>
                </>
              ) : (
                <>
                  <Terminal className="w-4 h-4" />
                  <Copy className="w-3 h-3" />
                  <span>SSH</span>
                </>
              )}
            </button>
          </Tooltip>
        </div>
      </div>
    </div>
  );
};

export default DeviceHeader;