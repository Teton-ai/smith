'use client';

import React, { useState, useEffect } from 'react';
import {
  Wifi,
  WifiOff,
  AlertTriangle,
  CheckCircle,
  XCircle,
  RefreshCw,
  Activity,
  Cpu,
  Battery,
  Thermometer,
  Settings,
  Search,
  Download,
  ChevronDown,
  GitBranch,
  Clock,
  Home,
  Layers,
  BarChart3,
  Users,
  Bell,
  Menu,
  X,
  MapPin,
  Zap,
  HardDrive
} from 'lucide-react';
import useSmithAPI from "@/app/hooks/smith-api";

interface DashboardData {
  total_count: number,
  online_count: number,
  offline_count: number,
  outdated_count: number,
  archived_count: number,
}

const AdminPanel = () => {
  const [activeTab, setActiveTab] = useState('dashboard');
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [devices, setDevices] = useState([]);
  const [filteredDevices, setFilteredDevices] = useState([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [filterStatus, setFilterStatus] = useState('all');
  const [lastUpdate, setLastUpdate] = useState(new Date());
  const { callAPI, loading, error } = useSmithAPI();
  const [dashboardData, setDashboardData] = useState<DashboardData | null>(null);

  useEffect(() => {
    const fetchDashboard = async () => {
      const data = await callAPI<DashboardData>('GET', '/dashboard');
      if (data) {
        setDashboardData(data);
      }
    };
    fetchDashboard();
  }, [callAPI]);

  // Generate sample device data
  useEffect(() => {
    const generateDevices = () => {
      const deviceTypes = ['Sensor', 'Gateway', 'Controller', 'Monitor', 'Actuator'];
      const locations = ['Building A', 'Building B', 'Warehouse', 'Factory Floor', 'Office Complex'];
      const statuses = ['online', 'offline', 'maintenance'];

      return Array.from({ length: 85 }, (_, i) => ({
        id: `IOT-${String(i + 1).padStart(4, '0')}`,
        name: `${deviceTypes[i % deviceTypes.length]} ${i + 1}`,
        type: deviceTypes[i % deviceTypes.length],
        location: locations[i % locations.length],
        status: statuses[Math.floor(Math.random() * 100) < 85 ? 0 : Math.floor(Math.random() * 2) + 1],
        lastSeen: new Date(Date.now() - Math.random() * 7 * 24 * 60 * 60 * 1000),
        firmwareVersion: Math.random() > 0.3 ? '2.1.4' : '2.0.1',
        latestVersion: '2.1.4',
        batteryLevel: Math.floor(Math.random() * 100),
        temperature: Math.floor(Math.random() * 40) + 20,
        cpuUsage: Math.floor(Math.random() * 100),
        memoryUsage: Math.floor(Math.random() * 100),
        uptime: Math.floor(Math.random() * 30) + 1
      }));
    };

    const deviceData = generateDevices();
    setDevices(deviceData);
    setFilteredDevices(deviceData);
  }, []);

  // Filter devices based on search and status
  useEffect(() => {
    let filtered = devices;

    if (searchTerm) {
      filtered = filtered.filter(device =>
        device.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
        device.id.toLowerCase().includes(searchTerm.toLowerCase()) ||
        device.location.toLowerCase().includes(searchTerm.toLowerCase())
      );
    }

    if (filterStatus !== 'all') {
      filtered = filtered.filter(device => device.status === filterStatus);
    }

    setFilteredDevices(filtered);
  }, [devices, searchTerm, filterStatus]);

  const stats = {
    total: devices.length,
    online: devices.filter(d => d.status === 'online').length,
    offline: devices.filter(d => d.status === 'offline').length,
    maintenance: devices.filter(d => d.status === 'maintenance').length,
    outdated: devices.filter(d => d.firmwareVersion !== d.latestVersion).length,
    lowBattery: devices.filter(d => d.batteryLevel < 20).length,
    highTemp: devices.filter(d => d.temperature > 35).length
  };

  const getHealthScore = () => {
    const onlineRatio = stats.online / stats.total;
    const updatedRatio = (stats.total - stats.outdated) / stats.total;
    const batteryRatio = (stats.total - stats.lowBattery) / stats.total;
    return Math.round((onlineRatio + updatedRatio + batteryRatio) / 3 * 100);
  };

  const getStatusColor = (status) => {
    switch (status) {
      case 'online': return 'text-green-700 bg-green-50 border-green-200';
      case 'offline': return 'text-red-700 bg-red-50 border-red-200';
      case 'maintenance': return 'text-yellow-700 bg-yellow-50 border-yellow-200';
      default: return 'text-gray-700 bg-gray-50 border-gray-200';
    }
  };

  const getStatusIcon = (status) => {
    switch (status) {
      case 'online': return <CheckCircle className="w-3 h-3" />;
      case 'offline': return <XCircle className="w-3 h-3" />;
      case 'maintenance': return <Settings className="w-3 h-3" />;
      default: return <AlertTriangle className="w-3 h-3" />;
    }
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

  const refreshDashboard = () => {
    setLastUpdate(new Date());
  };

  const healthScore = getHealthScore();

  const sidebarItems = [
    { id: 'dashboard', label: 'Dashboard', icon: Home },
    { id: 'devices', label: 'Devices', icon: Cpu },
    { id: 'distributions', label: 'Distributions', icon: Layers },
    { id: 'analytics', label: 'Analytics', icon: BarChart3 },
    { id: 'users', label: 'Users', icon: Users },
    { id: 'notifications', label: 'Notifications', icon: Bell },
  ];

  const renderDashboard = () => (
    <div className="space-y-6">
      {/* Status Cards */}
      <div className="grid grid-cols-1 sm:grid-cols-1 lg:grid-cols-3 gap-4">
        <div className="border border-gray-200 rounded-lg p-4 bg-white">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2">
              <div className="w-2 h-2 bg-green-500 rounded-full"></div>
              <span className="text-sm font-medium text-gray-900">Online</span>
            </div>
            <span className="text-2xl font-semibold text-gray-900">{dashboardData?.online_count}</span>
          </div>
          <div className="mt-1 text-xs text-gray-500">
            {Math.round(dashboardData?.online_count/dashboardData?.total_count*100)}% of fleet
          </div>
        </div>

        <div className="border border-gray-200 rounded-lg p-4 bg-white">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2">
              <div className="w-2 h-2 bg-red-500 rounded-full"></div>
              <span className="text-sm font-medium text-gray-900">Offline</span>
            </div>
            <span className="text-2xl font-semibold text-gray-900">{dashboardData?.offline_count}</span>
          </div>
          <div className="mt-1 text-xs text-gray-500">
            {stats.offline > 0 ? 'Needs attention' : 'All operational'}
          </div>
        </div>

        <div className="border border-gray-200 rounded-lg p-4 bg-white">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2">
              <div className="w-2 h-2 bg-yellow-500 rounded-full"></div>
              <span className="text-sm font-medium text-gray-900">Updates</span>
            </div>
            <span className="text-2xl font-semibold text-gray-900">{dashboardData?.outdated_count}</span>
          </div>
          <div className="mt-1 text-xs text-gray-500">
            {dashboardData?.outdated_count > 0 ? 'Pending deployment' : 'All up to date'}
          </div>
        </div>
      </div>

      {/* Alerts */}
      {(stats.offline > 0 || stats.outdated > 0 || stats.lowBattery > 0) && (
        <div className="border border-yellow-200 rounded-lg p-4 bg-yellow-50">
          <div className="flex items-start space-x-3">
            <AlertTriangle className="w-5 h-5 text-yellow-600 mt-0.5 flex-shrink-0" />
            <div className="flex-1">
              <h3 className="text-sm font-medium text-yellow-800">Action Required</h3>
              <div className="mt-2 text-sm text-yellow-700">
                <ul className="space-y-1">
                  {stats.offline > 0 && (
                    <li>• {stats.offline} device{stats.offline > 1 ? 's' : ''} offline - check connectivity</li>
                  )}
                  {stats.outdated > 0 && (
                    <li>• {stats.outdated} device{stats.outdated > 1 ? 's' : ''} need firmware updates</li>
                  )}
                  {stats.lowBattery > 0 && (
                    <li>• {stats.lowBattery} device{stats.lowBattery > 1 ? 's' : ''} have low battery</li>
                  )}
                </ul>
              </div>
              <div className="mt-3 flex space-x-2">
                {stats.offline > 0 && (
                  <button className="text-xs px-2 py-1 bg-red-600 text-white rounded hover:bg-red-700">
                    View Offline
                  </button>
                )}
                {stats.outdated > 0 && (
                  <button className="text-xs px-2 py-1 bg-blue-600 text-white rounded hover:bg-blue-700">
                    Deploy Updates
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Recent Activity */}
      <div className="border border-gray-200 rounded-lg bg-white">
        <div className="p-4 border-b border-gray-200">
          <h3 className="text-lg font-medium text-gray-900">Recent Activity</h3>
        </div>
        <div className="p-4 space-y-3">
          <div className="flex items-center space-x-3 text-sm">
            <div className="w-2 h-2 bg-green-500 rounded-full"></div>
            <span className="text-gray-600">IOT-0042 came online</span>
            <span className="text-gray-400">2 minutes ago</span>
          </div>
          <div className="flex items-center space-x-3 text-sm">
            <div className="w-2 h-2 bg-blue-500 rounded-full"></div>
            <span className="text-gray-600">Firmware update deployed to 12 devices</span>
            <span className="text-gray-400">15 minutes ago</span>
          </div>
          <div className="flex items-center space-x-3 text-sm">
            <div className="w-2 h-2 bg-red-500 rounded-full"></div>
            <span className="text-gray-600">IOT-0018 went offline</span>
            <span className="text-gray-400">1 hour ago</span>
          </div>
        </div>
      </div>
    </div>
  );

  const renderDevices = () => (
    <div className="space-y-6">
      {/* Filters */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center space-x-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
            <input
              type="text"
              placeholder="Search devices..."
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm"
            />
          </div>

          <div className="relative">
            <select
              value={filterStatus}
              onChange={(e) => setFilterStatus(e.target.value)}
              className="appearance-none pl-3 pr-8 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm bg-white"
            >
              <option value="all">All status</option>
              <option value="online">Online</option>
              <option value="offline">Offline</option>
              <option value="maintenance">Maintenance</option>
            </select>
            <ChevronDown className="absolute right-2 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4 pointer-events-none" />
          </div>
        </div>

        <div className="mt-4 sm:mt-0 flex items-center space-x-3">
          <span className="text-sm text-gray-500">
            {filteredDevices.length} device{filteredDevices.length !== 1 ? 's' : ''}
          </span>
          <button className="px-3 py-2 bg-blue-600 text-white text-sm rounded-md hover:bg-blue-700">
            Add Device
          </button>
        </div>
      </div>

      {/* Device List */}
      <div className="border border-gray-200 rounded-lg overflow-hidden bg-white">
        <div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
          <div className="grid grid-cols-12 gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide">
            <div className="col-span-4">Device</div>
            <div className="col-span-2">Status</div>
            <div className="col-span-2">Location</div>
            <div className="col-span-1">Version</div>
            <div className="col-span-2">Health</div>
            <div className="col-span-1">Updated</div>
          </div>
        </div>

        <div className="divide-y divide-gray-200">
          {filteredDevices.slice(0, 25).map((device) => (
            <div key={device.id} className="px-4 py-3 hover:bg-gray-50">
              <div className="grid grid-cols-12 gap-4 items-center">
                <div className="col-span-4">
                  <div className="flex items-center space-x-3">
                    <Cpu className="w-4 h-4 text-gray-400 flex-shrink-0" />
                    <div className="min-w-0">
                      <div className="text-sm font-medium text-gray-900 truncate">
                        {device.name}
                      </div>
                      <div className="text-xs text-gray-500 font-mono">
                        {device.id}
                      </div>
                    </div>
                  </div>
                </div>

                <div className="col-span-2">
                  <span className={`inline-flex items-center px-2 py-1 rounded-full text-xs font-medium border ${getStatusColor(device.status)}`}>
                    {getStatusIcon(device.status)}
                    <span className="ml-1.5 capitalize">{device.status}</span>
                  </span>
                </div>

                <div className="col-span-2 text-sm text-gray-600">
                  {device.location}
                </div>

                <div className="col-span-1">
                  <div className="flex items-center space-x-1">
                    <GitBranch className="w-3 h-3 text-gray-400" />
                    <span className="text-xs font-mono text-gray-600">
                      {device.firmwareVersion}
                    </span>
                    {device.firmwareVersion !== device.latestVersion && (
                      <span className="inline-flex items-center px-1.5 py-0.5 rounded text-xs font-medium bg-yellow-100 text-yellow-800 border border-yellow-200">
                        <Download className="w-2.5 h-2.5 mr-0.5" />
                        Update
                      </span>
                    )}
                  </div>
                </div>

                <div className="col-span-2">
                  <div className="flex items-center space-x-3">
                    <div className="flex items-center space-x-1">
                      <Battery className="w-3 h-3 text-gray-400" />
                      <span className={`text-xs ${device.batteryLevel < 20 ? 'text-red-600 font-medium' : 'text-gray-600'}`}>
                        {device.batteryLevel}%
                      </span>
                    </div>
                    <div className="flex items-center space-x-1">
                      <Thermometer className="w-3 h-3 text-gray-400" />
                      <span className={`text-xs ${device.temperature > 35 ? 'text-red-600 font-medium' : 'text-gray-600'}`}>
                        {device.temperature}°
                      </span>
                    </div>
                  </div>
                </div>

                <div className="col-span-1">
                  <div className="flex items-center space-x-1">
                    <Clock className="w-3 h-3 text-gray-400" />
                    <span className="text-xs text-gray-500">
                      {formatTimeAgo(device.lastSeen)}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );

  const renderDistributions = () => (
    <div className="space-y-6">
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Device Types */}
        <div className="border border-gray-200 rounded-lg bg-white p-6">
          <h3 className="text-lg font-medium text-gray-900 mb-4">Device Types</h3>
          <div className="space-y-3">
            {['Sensor', 'Gateway', 'Controller', 'Monitor', 'Actuator'].map(type => {
              const count = devices.filter(d => d.type === type).length;
              const percentage = (count / devices.length * 100).toFixed(1);
              return (
                <div key={type} className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <div className="w-3 h-3 bg-blue-500 rounded"></div>
                    <span className="text-sm text-gray-700">{type}</span>
                  </div>
                  <div className="flex items-center space-x-2">
                    <span className="text-sm font-medium text-gray-900">{count}</span>
                    <span className="text-xs text-gray-500">({percentage}%)</span>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* Locations */}
        <div className="border border-gray-200 rounded-lg bg-white p-6">
          <h3 className="text-lg font-medium text-gray-900 mb-4">Locations</h3>
          <div className="space-y-3">
            {['Building A', 'Building B', 'Warehouse', 'Factory Floor', 'Office Complex'].map(location => {
              const count = devices.filter(d => d.location === location).length;
              const percentage = (count / devices.length * 100).toFixed(1);
              return (
                <div key={location} className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <MapPin className="w-3 h-3 text-gray-400" />
                    <span className="text-sm text-gray-700">{location}</span>
                  </div>
                  <div className="flex items-center space-x-2">
                    <span className="text-sm font-medium text-gray-900">{count}</span>
                    <span className="text-xs text-gray-500">({percentage}%)</span>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* Firmware Versions */}
        <div className="border border-gray-200 rounded-lg bg-white p-6">
          <h3 className="text-lg font-medium text-gray-900 mb-4">Firmware Distribution</h3>
          <div className="space-y-3">
            {['2.1.4', '2.0.1'].map(version => {
              const count = devices.filter(d => d.firmwareVersion === version).length;
              const percentage = (count / devices.length * 100).toFixed(1);
              const isLatest = version === '2.1.4';
              return (
                <div key={version} className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <div className={`w-3 h-3 rounded ${isLatest ? 'bg-green-500' : 'bg-yellow-500'}`}></div>
                    <span className="text-sm text-gray-700 font-mono">{version}</span>
                    {isLatest && <span className="text-xs text-green-600 font-medium">Latest</span>}
                  </div>
                  <div className="flex items-center space-x-2">
                    <span className="text-sm font-medium text-gray-900">{count}</span>
                    <span className="text-xs text-gray-500">({percentage}%)</span>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* Status Distribution */}
        <div className="border border-gray-200 rounded-lg bg-white p-6">
          <h3 className="text-lg font-medium text-gray-900 mb-4">Status Distribution</h3>
          <div className="space-y-3">
            {[
              { status: 'online', color: 'bg-green-500', count: stats.online },
              { status: 'offline', color: 'bg-red-500', count: stats.offline },
              { status: 'maintenance', color: 'bg-yellow-500', count: stats.maintenance }
            ].map(({ status, color, count }) => {
              const percentage = (count / devices.length * 100).toFixed(1);
              return (
                <div key={status} className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <div className={`w-3 h-3 rounded ${color}`}></div>
                    <span className="text-sm text-gray-700 capitalize">{status}</span>
                  </div>
                  <div className="flex items-center space-x-2">
                    <span className="text-sm font-medium text-gray-900">{count}</span>
                    <span className="text-xs text-gray-500">({percentage}%)</span>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );

  const renderContent = () => {
    switch (activeTab) {
      case 'dashboard':
        return renderDashboard();
      case 'devices':
        return renderDevices();
      case 'distributions':
        return renderDistributions();
      case 'analytics':
        return <div className="text-center py-12 text-gray-500">Analytics coming soon...</div>;
      case 'users':
        return <div className="text-center py-12 text-gray-500">User management coming soon...</div>;
      case 'notifications':
        return <div className="text-center py-12 text-gray-500">Notifications coming soon...</div>;
      default:
        return renderDashboard();
    }
  };

  return (
    <div className="min-h-screen bg-gray-50 flex">
      {/* Sidebar */}
      <div className={`${sidebarOpen ? 'translate-x-0' : '-translate-x-full'} fixed inset-y-0 left-0 z-50 w-64 bg-white border-r border-gray-200 transform transition-transform duration-300 ease-in-out lg:translate-x-0 lg:static lg:inset-0`}>
        <div className="flex items-center justify-between h-16 px-4 border-b border-gray-200">
          <div className="flex items-center space-x-2">
            <HardDrive className="w-6 h-6 text-blue-600" />
            <span className="text-lg font-semibold text-gray-900">IoT Admin</span>
          </div>
          <button
            onClick={() => setSidebarOpen(false)}
            className="lg:hidden p-1 rounded-md text-gray-400 hover:text-gray-500"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <nav className="mt-5 px-2">
          <div className="space-y-1">
            {sidebarItems.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.id}
                  onClick={() => {
                    setActiveTab(item.id);
                    setSidebarOpen(false);
                  }}
                  className={`${
                    activeTab === item.id
                      ? 'bg-blue-50 border-blue-500 text-blue-700'
                      : 'border-transparent text-gray-600 hover:bg-gray-50 hover:text-gray-900'
                  } group flex items-center px-2 py-2 text-sm font-medium rounded-md border-l-4 w-full`}
                >
                  <Icon className="mr-3 h-5 w-5" />
                  {item.label}
                </button>
              );
            })}
          </div>
        </nav>
      </div>

      {/* Main Content */}
      <div className="flex-1 lg:pl-0">
        {/* Header */}
        <div className="bg-white border-b border-gray-200">
          <div className="px-4 sm:px-6 lg:px-8">
            <div className="flex justify-between items-center h-16">
              <div className="flex items-center">
                <button
                  onClick={() => setSidebarOpen(true)}
                  className="lg:hidden p-2 rounded-md text-gray-400 hover:text-gray-500"
                >
                  <Menu className="w-5 h-5" />
                </button>
                <h1 className="ml-2 lg:ml-0 text-2xl font-semibold text-gray-900 capitalize">
                  {activeTab}
                </h1>
              </div>

              <div className="flex items-center space-x-4">
                <span className="text-sm text-gray-500">
                  Updated {formatTimeAgo(lastUpdate)} ago
                </span>
                <button
                  onClick={refreshDashboard}
                  className="flex items-center px-3 py-1.5 text-sm border border-gray-300 rounded-md hover:bg-gray-50 text-gray-700"
                >
                  <RefreshCw className="w-4 h-4 mr-1.5" />
                  Refresh
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* Page Content */}
        <main className="px-4 sm:px-6 lg:px-8 py-6">
          {renderContent()}
        </main>
      </div>

      {/* Sidebar Overlay */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 z-40 bg-gray-600 bg-opacity-75 lg:hidden"
          onClick={() => setSidebarOpen(false)}
        />
      )}
    </div>
  );
};

export default AdminPanel;
