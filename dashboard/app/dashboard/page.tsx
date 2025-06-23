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
import PrivateLayout from "@/app/layouts/PrivateLayout";

interface DashboardData {
  total_count: number,
  online_count: number,
  offline_count: number,
  outdated_count: number,
  archived_count: number,
}

const AdminPanel = () => {
  const { callAPI } = useSmithAPI();
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

  const stats = {
    total: 0,
    online: 0,
    offline: 0,
    maintenance: 0,
    outdated: 0,
    lowBattery: 0,
    highTemp: 0
  };

  return (
    <PrivateLayout id="dashboard">
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
    </PrivateLayout>
  );
};

export default AdminPanel;
