'use client';

import React, { useState, useEffect } from 'react';
import { useParams, useRouter } from 'next/navigation';
import {
  Cpu,
  ChevronRight,
  Package,
  GitBranch,
  Shield,
  Zap,
  HardDrive,
  CheckCircle,
  AlertTriangle,
  Info,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

interface Package {
  id: number;
  name: string;
  architecture: string;
  version: string;
  file: string;
  created_at: string;
}

const PackagesPage = () => {
  const params = useParams();
  const router = useRouter();
  const { callAPI } = useSmithAPI();
  const [packages, setPackages] = useState<Package[]>([]);
  const [loading, setLoading] = useState(true);
  const [releaseId, setReleaseId] = useState<number | null>(null);

  const serial = params.serial as string;

  useEffect(() => {
    const fetchPackages = async () => {
      setLoading(true);
      try {
        const deviceData = await callAPI('GET', `/devices/${serial}`);
        if (deviceData?.release_id) {
          setReleaseId(deviceData.release_id);
          
          const packagesData = await callAPI('GET', `/releases/${deviceData.release_id}/packages`);
          if (packagesData && Array.isArray(packagesData)) {
            setPackages(packagesData);
          }
        }
      } catch (error) {
        console.error('Error fetching packages:', error);
      } finally {
        setLoading(false);
      }
    };
    fetchPackages();
  }, [callAPI, serial]);


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
          <span className="text-gray-900 font-medium">Packages</span>
        </div>

        {/* Device Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center space-x-4">
            <div className="p-3 bg-gray-100 rounded-lg">
              <Package className="w-8 h-8 text-gray-600" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-gray-900">{serial}</h1>
              <p className="text-gray-600 mt-1">Package Management</p>
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
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
            >
              Packages
            </button>
          </nav>
        </div>

        {/* Packages List */}
        <div className="bg-white rounded-lg border border-gray-200">
          <div className="p-6 border-b border-gray-200">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-lg font-semibold text-gray-900">Device Packages</h3>
                <p className="text-sm text-gray-700 mt-1">
                  {packages.length} packages installed{releaseId && ` (Release ${releaseId})`}
                </p>
              </div>
            </div>
          </div>
          {loading ? (
            <div className="p-6 text-center text-gray-500">
              Loading packages...
            </div>
          ) : (
            <div className="divide-y divide-gray-200">
              {packages.map((pkg) => (
                <div key={pkg.id} className="p-6 hover:bg-gray-50">
                  <div className="flex items-start justify-between">
                    <div className="flex-1">
                      <div className="flex items-center space-x-3 mb-2">
                        <h4 className="font-mono font-semibold text-gray-900">{pkg.name}</h4>
                        <span className="font-mono text-sm text-gray-600">{pkg.version}</span>
                        <span className="inline-flex px-2 py-1 text-xs font-medium rounded-full bg-gray-100 text-gray-700">
                          {pkg.architecture}
                        </span>
                      </div>
                      <div className="flex items-center space-x-6 text-sm text-gray-600">
                        <span className="font-mono">{pkg.file}</span>
                        <span>Created: {new Date(pkg.created_at).toLocaleDateString()}</span>
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </PrivateLayout>
  );
};

export default PackagesPage;