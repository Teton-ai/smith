'use client';

import React, { useState, useEffect } from 'react';
import { useParams, useRouter } from 'next/navigation';
import {
  Cpu,
  ChevronRight,
  FileText,
  AlertTriangle,
  Info,
  CheckCircle,
  XCircle,
  Clock,
  Search,
  Filter,
  Download,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";

// Mock logs data
const mockLogs = [
  {
    id: 1,
    timestamp: "2025-01-13T14:03:45.123Z",
    level: "info",
    service: "smith-agent",
    message: "Telemetry data successfully transmitted to server",
    details: {
      endpoint: "https://api.smith.teton.com/telemetry",
      responseTime: "142ms",
      statusCode: 200
    }
  },
  {
    id: 2,
    timestamp: "2025-01-13T14:02:30.456Z", 
    level: "warning",
    service: "smith-network",
    message: "Network interface eth0 disconnected, attempting reconnection",
    details: {
      interface: "eth0",
      lastIP: "192.168.1.100",
      retryCount: 1
    }
  },
  {
    id: 3,
    timestamp: "2025-01-13T14:01:15.789Z",
    level: "error",
    service: "smith-agent",
    message: "Failed to authenticate with update server", 
    details: {
      endpoint: "https://updates.smith.teton.com",
      errorCode: "AUTH_EXPIRED",
      nextRetry: "2025-01-13T14:06:15.789Z"
    }
  },
  {
    id: 4,
    timestamp: "2025-01-13T14:00:00.000Z",
    level: "info", 
    service: "systemd",
    message: "Smith agent service started successfully",
    details: {
      pid: 1234,
      version: "2.35.0",
      startTime: "245ms"
    }
  },
  {
    id: 5,
    timestamp: "2025-01-13T13:59:45.321Z",
    level: "info",
    service: "smith-telemetry", 
    message: "System metrics collected: CPU 23.5%, Memory 67.2%, Disk 45.8%",
    details: {
      cpu_usage: 23.5,
      memory_usage: 67.2,
      disk_usage: 45.8,
      temperature: 42.3
    }
  },
  {
    id: 6,
    timestamp: "2025-01-13T13:58:30.654Z",
    level: "warning",
    service: "kernel",
    message: "Temperature threshold warning: 65°C detected",
    details: {
      currentTemp: 65.2,
      threshold: 65.0,
      sensor: "thermal_zone0"
    }
  },
  {
    id: 7,
    timestamp: "2025-01-13T13:57:15.987Z", 
    level: "info",
    service: "smith-network",
    message: "Network connectivity restored on interface usb0",
    details: {
      interface: "usb0", 
      newIP: "fe80::9553:3879:1cff:f7c1",
      connectionTime: "2.3s"
    }
  }
];

const LogsPage = () => {
  const params = useParams();
  const router = useRouter();
  const [logs, setLogs] = useState(mockLogs);
  const [filteredLogs, setFilteredLogs] = useState(mockLogs);
  const [searchTerm, setSearchTerm] = useState('');
  const [selectedLevel, setSelectedLevel] = useState('all');
  const [selectedService, setSelectedService] = useState('all');
  const [loading, setLoading] = useState(false);

  const serial = params.serial as string;

  const levels = [
    { id: 'all', name: 'All Levels', count: logs.length },
    { id: 'error', name: 'Error', count: logs.filter(l => l.level === 'error').length },
    { id: 'warning', name: 'Warning', count: logs.filter(l => l.level === 'warning').length },
    { id: 'info', name: 'Info', count: logs.filter(l => l.level === 'info').length }
  ];

  const services = [
    { id: 'all', name: 'All Services' },
    ...Array.from(new Set(logs.map(l => l.service))).map(service => ({
      id: service,
      name: service
    }))
  ];

  // Filter logs based on search term, level, and service
  useEffect(() => {
    let filtered = logs;

    if (searchTerm) {
      filtered = filtered.filter(log =>
        log.message.toLowerCase().includes(searchTerm.toLowerCase()) ||
        log.service.toLowerCase().includes(searchTerm.toLowerCase())
      );
    }

    if (selectedLevel !== 'all') {
      filtered = filtered.filter(log => log.level === selectedLevel);
    }

    if (selectedService !== 'all') {
      filtered = filtered.filter(log => log.service === selectedService);
    }

    setFilteredLogs(filtered);
  }, [logs, searchTerm, selectedLevel, selectedService]);

  const getLevelIcon = (level: string) => {
    switch (level) {
      case 'error': return <XCircle className="w-4 h-4 text-red-500" />;
      case 'warning': return <AlertTriangle className="w-4 h-4 text-yellow-500" />;
      case 'info': return <Info className="w-4 h-4 text-blue-500" />;
      default: return <CheckCircle className="w-4 h-4 text-gray-500" />;
    }
  };

  const getLevelColor = (level: string) => {
    switch (level) {
      case 'error': return 'text-red-600 bg-red-50 border-red-200';
      case 'warning': return 'text-yellow-600 bg-yellow-50 border-yellow-200';
      case 'info': return 'text-blue-600 bg-blue-50 border-blue-200';
      default: return 'text-gray-600 bg-gray-50 border-gray-200';
    }
  };

  const formatTimestamp = (timestamp: string) => {
    return new Date(timestamp).toLocaleString('en-US', {
      month: 'short',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      fractionalSecondDigits: 3
    });
  };

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
          <button 
            onClick={() => router.push(`/devices/${serial}`)}
            className="hover:text-gray-700 transition-colors cursor-pointer"
          >
            {serial}
          </button>
          <ChevronRight className="w-4 h-4" />
          <span className="text-gray-900 font-medium">Logs</span>
        </div>

        {/* Device Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center space-x-4">
            <div className="p-3 bg-gray-100 rounded-lg">
              <FileText className="w-8 h-8 text-gray-600" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-gray-900">{serial}</h1>
              <p className="text-gray-600 mt-1">System Logs</p>
            </div>
          </div>
        </div>

        {/* Tabs */}
        <div className="border-b border-gray-200">
          <nav className="-mb-px flex space-x-8">
            <button
              onClick={() => router.push(`/devices/${serial}`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
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
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
            >
              Logs
            </button>
          </nav>
        </div>

        {/* Filters */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4">
            <div className="flex flex-col sm:flex-row sm:items-center gap-4">
              {/* Search */}
              <div className="relative">
                <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
                <input
                  type="text"
                  placeholder="Search logs..."
                  value={searchTerm}
                  onChange={(e) => setSearchTerm(e.target.value)}
                  className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
                />
              </div>

              {/* Level Filter */}
              <select
                value={selectedLevel}
                onChange={(e) => setSelectedLevel(e.target.value)}
                className="border border-gray-300 rounded-md px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
              >
                {levels.map(level => (
                  <option key={level.id} value={level.id}>
                    {level.name} ({level.count})
                  </option>
                ))}
              </select>

              {/* Service Filter */}
              <select
                value={selectedService}
                onChange={(e) => setSelectedService(e.target.value)}
                className="border border-gray-300 rounded-md px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
              >
                {services.map(service => (
                  <option key={service.id} value={service.id}>
                    {service.name}
                  </option>
                ))}
              </select>
            </div>

            <div className="flex items-center space-x-3">
              <span className="text-sm text-gray-500">
                {filteredLogs.length} log{filteredLogs.length !== 1 ? 's' : ''}
              </span>
              <button className="px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-50 transition-colors flex items-center space-x-2">
                <Download className="w-4 h-4" />
                <span>Export</span>
              </button>
            </div>
          </div>
        </div>

        {/* Logs List */}
        <div className="space-y-6">
          <div className="bg-white rounded-lg border border-gray-200">
            <div className="p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">System Logs</h3>
              <p className="text-sm text-gray-700 mt-1">Real-time logs from device services and system components</p>
            </div>
            <div className="divide-y divide-gray-200">
              {filteredLogs.map((log) => (
                <div key={log.id} className="p-6 hover:bg-gray-50">
                  <div className="flex items-start space-x-4">
                    <div className="flex-shrink-0 mt-1">
                      {getLevelIcon(log.level)}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center space-x-3 mb-2">
                        <span className={`inline-flex px-2 py-1 text-xs font-medium rounded-full border ${getLevelColor(log.level)}`}>
                          {log.level}
                        </span>
                        <span className="inline-flex px-2 py-1 text-xs font-medium bg-gray-100 text-gray-700 rounded font-mono">
                          {log.service}
                        </span>
                        <div className="flex items-center space-x-1 text-xs text-gray-500">
                          <Clock className="w-3 h-3" />
                          <span>{formatTimestamp(log.timestamp)}</span>
                        </div>
                      </div>
                      
                      <p className="text-gray-900 mb-3 font-mono text-sm">{log.message}</p>
                      
                      {log.details && (
                        <details className="mt-3">
                          <summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800 font-medium">
                            Show details
                          </summary>
                          <div className="mt-2 pl-4 border-l-2 border-gray-200">
                            <pre className="text-xs bg-gray-50 text-gray-700 p-3 rounded overflow-x-auto">
                              {JSON.stringify(log.details, null, 2)}
                            </pre>
                          </div>
                        </details>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default LogsPage;