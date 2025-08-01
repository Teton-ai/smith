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
    total: dashboardData?.total_count || 0,
    online: dashboardData?.online_count || 0,
    offline: dashboardData?.offline_count || 0,
    maintenance: 0,
    outdated: dashboardData?.outdated_count || 0,
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
              {dashboardData?.total_count ? Math.round((dashboardData.online_count / dashboardData.total_count) * 100) : 0}% of fleet
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
              {dashboardData?.offline_count > 0 ? 'Needs attention' : 'All operational'}
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
      </div>
    </PrivateLayout>
  );
};

export default AdminPanel;
