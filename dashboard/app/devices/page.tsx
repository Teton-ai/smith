'use client';

import React, { useState, useEffect } from 'react';
import {
  AlertTriangle,
  CheckCircle,
  XCircle,
  Cpu,
  Battery,
  Thermometer,
  Search,
  Download,
  ChevronDown,
  GitBranch,
  Clock,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

interface Device {
  serial_number: string
  last_seen: string
}

const DevicesPage = () => {
  const { callAPI, loading, error } = useSmithAPI();
  const [devices, setDevices] = useState<Device[]>([]);
  const [filteredDevices, setFilteredDevices] = useState<Device[]>([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [filterStatus, setFilterStatus] = useState('all');

  useEffect(() => {
    const fetchDashboard = async () => {
      const data = await callAPI<Device[]>('GET', '/devices');
      if (data) {
        setDevices(data);
      }
    };
    fetchDashboard();
  }, [callAPI]);

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

  const getDeviceStatus = (device) => {
    const lastSeen = new Date(device.last_seen);
    const now = new Date();
    const diffMinutes = (now - lastSeen) / (1000 * 60);

    return diffMinutes <= 3 ? 'online' : 'offline';
  };

  const getStatusColor = (device) => {
    const status = getDeviceStatus(device);
    switch (status) {
      case 'online': return 'text-green-700 bg-green-50 border-green-200';
      case 'offline': return 'text-red-700 bg-red-50 border-red-200';
      default: return 'text-gray-700 bg-gray-50 border-gray-200';
    }
  };

  const getStatusIcon = (device) => {
    const status = getDeviceStatus(device);
    switch (status) {
      case 'online': return <CheckCircle className="w-3 h-3" />;
      case 'offline': return <XCircle className="w-3 h-3" />;
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

  return (
    <PrivateLayout id="devices">
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
              </select>
              <ChevronDown className="absolute right-2 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4 pointer-events-none" />
            </div>
          </div>

          <div className="mt-4 sm:mt-0 flex items-center space-x-3">
          <span className="text-sm text-gray-500">
            {filteredDevices.length} device{filteredDevices.length !== 1 ? 's' : ''}
          </span>
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
            {filteredDevices.map((device) => (
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
                          {device.serial_number}
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="col-span-2">
                    <span className={`inline-flex items-center px-2 py-1 rounded-full text-xs font-medium border ${getStatusColor(device)}`}>
                      {getStatusIcon(device)}
                      <span className="ml-1.5 capitalize">{getDeviceStatus(device)}</span>
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
    </PrivateLayout>
  );
};

export default DevicesPage;
