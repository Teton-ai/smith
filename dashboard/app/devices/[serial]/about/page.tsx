'use client';

import React, { useState, useEffect } from 'react';
import { useParams, useRouter } from 'next/navigation';
import {
  ChevronRight,
  XCircle,
  GitBranch,
  Tag,
  Globe,
  Clock,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";
import DeviceHeader from '../DeviceHeader';

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

const DeviceAboutPage = () => {
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


  if (loading) {
    return (
      <PrivateLayout id="devices">
        <div className="space-y-6">
          {/* Breadcrumb Skeleton */}
          <div className="flex items-center space-x-2">
            <div className="h-4 bg-gray-200 rounded w-16 animate-pulse" />
            <ChevronRight className="w-4 h-4 text-gray-300" />
            <div className="h-4 bg-gray-200 rounded w-32 animate-pulse" />
            <ChevronRight className="w-4 h-4 text-gray-300" />
            <div className="h-4 bg-gray-200 rounded w-16 animate-pulse" />
          </div>

          {/* Header Skeleton */}
          <div className="bg-white rounded-lg border border-gray-200 p-6">
            <div className="h-8 bg-gray-200 rounded w-48 animate-pulse mb-4" />
            <div className="space-y-3">
              {[...Array(8)].map((_, i) => (
                <div key={i} className="h-12 bg-gray-100 rounded animate-pulse" />
              ))}
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
          <span className="text-gray-900 font-medium">About</span>
        </div>

        {/* Device Header */}
        <DeviceHeader device={device} serial={serial} />

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
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
            >
              About
            </button>
          </nav>
        </div>

        {/* About Content */}
        <div className="space-y-6">

          {/* System Information */}
          <div className="bg-white rounded-lg border border-gray-200 p-6">
            <h3 className="text-lg font-semibold text-gray-900 mb-4">System Information</h3>
            <div className="space-y-3">
              {/* Device Model */}
              {device.system_info?.device_tree?.model && (
                <div className="flex justify-between">
                  <span className="text-gray-700">Model</span>
                  <span className="text-gray-900">{device.system_info.device_tree.model}</span>
                </div>
              )}

              {/* Operating System */}
              {device.system_info?.os_release?.pretty_name && (
                <div className="flex justify-between">
                  <span className="text-gray-700">Operating System</span>
                  <span className="font-mono text-sm text-gray-900">{device.system_info.os_release.pretty_name}</span>
                </div>
              )}

              {/* Kernel Version */}
              {device.system_info?.proc?.version && (
                <div className="flex justify-between">
                  <span className="text-gray-700">Kernel</span>
                  <span className="font-mono text-sm text-gray-900">{device.system_info.proc.version}</span>
                </div>
              )}

              {/* Distribution */}
              {device.release && (
                <div className="flex justify-between">
                  <span className="text-gray-700 flex items-center">
                    <GitBranch className="w-4 h-4 text-gray-400 mr-2" />
                    Distribution
                  </span>
                  <button
                    onClick={() => router.push(`/distributions/${device.release.distribution_id}`)}
                    className="font-mono text-sm text-blue-600 hover:text-blue-800 hover:underline cursor-pointer transition-colors"
                  >
                    {device.release.distribution_name}
                  </button>
                </div>
              )}

              {/* Current Release */}
              {device.release && (
                <div className="flex justify-between">
                  <span className="text-gray-700 flex items-center">
                    <Tag className="w-4 h-4 text-gray-400 mr-2" />
                    Current Release
                  </span>
                  <button
                    onClick={() => router.push(`/releases/${device.release.id}`)}
                    className="font-mono text-sm text-blue-600 hover:text-blue-800 hover:underline cursor-pointer transition-colors"
                  >
                    {device.release.version}
                  </button>
                </div>
              )}

              {/* Target Release */}
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

              {/* Agent Version */}
              {device.system_info?.smith?.version && (
                <div className="flex justify-between">
                  <span className="text-gray-700">Agent</span>
                  <span className="font-mono text-sm text-gray-900">{device.system_info.smith.version}</span>
                </div>
              )}

              {/* Boot Time */}
              {device.system_info?.proc?.stat?.btime && (
                <div className="flex justify-between">
                  <span className="text-gray-700">Boot Time</span>
                  <span className="text-sm text-gray-900">
                    {new Date(device.system_info.proc.stat.btime * 1000).toLocaleString()}
                  </span>
                </div>
              )}

            </div>
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default DeviceAboutPage;