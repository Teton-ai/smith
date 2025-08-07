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

// Mock packages data for current release
const mockCurrentRelease = {
  id: 398,
  version: "2.35.0",
  name: "Stable Release 2.35.0",
  deployedAt: "2025-01-13T14:04:19Z",
  totalSize: "245 MB",
  packageCount: 23
};

const mockPackages = [
  {
    name: "smith-agent",
    version: "2.35.0", 
    category: "core",
    size: "45.2 MB",
    description: "Main Smith agent service for device management and telemetry",
    status: "healthy",
    lastUpdated: "2025-01-13T14:04:19Z",
    dependencies: ["systemd", "openssl", "curl"]
  },
  {
    name: "linux-tegra",
    version: "5.10.104-tegra",
    category: "kernel", 
    size: "89.1 MB",
    description: "NVIDIA Tegra Linux kernel with hardware acceleration support",
    status: "healthy",
    lastUpdated: "2025-01-13T14:04:19Z",
    dependencies: []
  },
  {
    name: "nvidia-l4t-core",
    version: "35.4.1",
    category: "system",
    size: "67.8 MB", 
    description: "NVIDIA Linux for Tegra core system components",
    status: "healthy",
    lastUpdated: "2025-01-13T14:04:19Z",
    dependencies: ["linux-tegra"]
  },
  {
    name: "smith-telemetry",
    version: "1.8.2",
    category: "service",
    size: "12.4 MB",
    description: "Telemetry collection and reporting service",
    status: "healthy", 
    lastUpdated: "2025-01-13T14:04:19Z",
    dependencies: ["smith-agent"]
  },
  {
    name: "smith-network",
    version: "2.1.5",
    category: "service",
    size: "8.9 MB",
    description: "Network management and connectivity service",
    status: "warning",
    lastUpdated: "2025-01-13T14:04:19Z",
    dependencies: ["networkmanager", "systemd-networkd"]
  },
  {
    name: "opencv-tegra",
    version: "4.6.0", 
    category: "library",
    size: "156.3 MB",
    description: "OpenCV computer vision library optimized for Tegra",
    status: "healthy",
    lastUpdated: "2025-01-13T14:04:19Z",
    dependencies: ["cuda-toolkit", "nvidia-l4t-core"]
  },
  {
    name: "cuda-toolkit",
    version: "11.8.89",
    category: "library",
    size: "89.7 MB",
    description: "NVIDIA CUDA Toolkit for GPU computing",
    status: "healthy",
    lastUpdated: "2025-01-13T14:04:19Z", 
    dependencies: ["nvidia-l4t-core"]
  }
];

const PackagesPage = () => {
  const params = useParams();
  const router = useRouter();
  const [currentRelease, setCurrentRelease] = useState(mockCurrentRelease);
  const [packages, setPackages] = useState(mockPackages);
  const [selectedCategory, setSelectedCategory] = useState('all');
  const [loading, setLoading] = useState(false);

  const serial = params.serial as string;

  const categories = [
    { id: 'all', name: 'All Packages', count: packages.length },
    { id: 'core', name: 'Core', count: packages.filter(p => p.category === 'core').length },
    { id: 'kernel', name: 'Kernel', count: packages.filter(p => p.category === 'kernel').length },
    { id: 'system', name: 'System', count: packages.filter(p => p.category === 'system').length },
    { id: 'service', name: 'Services', count: packages.filter(p => p.category === 'service').length },
    { id: 'library', name: 'Libraries', count: packages.filter(p => p.category === 'library').length }
  ];

  const filteredPackages = selectedCategory === 'all' 
    ? packages 
    : packages.filter(pkg => pkg.category === selectedCategory);

  const getCategoryIcon = (category: string) => {
    switch (category) {
      case 'core': return <Zap className="w-4 h-4" />;
      case 'kernel': return <Cpu className="w-4 h-4" />;
      case 'system': return <HardDrive className="w-4 h-4" />;
      case 'service': return <GitBranch className="w-4 h-4" />;
      case 'library': return <Package className="w-4 h-4" />;
      default: return <Package className="w-4 h-4" />;
    }
  };

  const getCategoryColor = (category: string) => {
    switch (category) {
      case 'core': return 'text-purple-600 bg-purple-100';
      case 'kernel': return 'text-red-600 bg-red-100'; 
      case 'system': return 'text-blue-600 bg-blue-100';
      case 'service': return 'text-green-600 bg-green-100';
      case 'library': return 'text-orange-600 bg-orange-100';
      default: return 'text-gray-600 bg-gray-100';
    }
  };

  const getStatusIcon = (status: string) => {
    switch (status) {
      case 'healthy': return <CheckCircle className="w-4 h-4 text-green-500" />;
      case 'warning': return <AlertTriangle className="w-4 h-4 text-yellow-500" />;
      default: return <Info className="w-4 h-4 text-gray-500" />;
    }
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'healthy': return 'text-green-600 bg-green-50 border-green-200';
      case 'warning': return 'text-yellow-600 bg-yellow-50 border-yellow-200';
      default: return 'text-gray-600 bg-gray-50 border-gray-200';
    }
  };

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
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
              onClick={() => router.push(`/devices/${serial}/releases`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              Releases
            </button>
            <button
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
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

        {/* Current Release Info */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-4">
              <div className="flex items-center space-x-2">
                <GitBranch className="w-5 h-5 text-gray-500" />
                <span className="font-mono text-lg font-semibold text-gray-900">{currentRelease.version}</span>
              </div>
              <span className="text-gray-600">{currentRelease.name}</span>
            </div>
            <div className="flex items-center space-x-6 text-sm text-gray-600">
              <div className="flex items-center space-x-1">
                <Package className="w-4 h-4" />
                <span>{currentRelease.packageCount} packages</span>
              </div>
              <div className="flex items-center space-x-1">
                <HardDrive className="w-4 h-4" />
                <span>{currentRelease.totalSize}</span>
              </div>
              <span>Deployed: {formatDate(currentRelease.deployedAt)}</span>
            </div>
          </div>
        </div>

        {/* Category Filter */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <h3 className="text-lg font-semibold text-gray-900 mb-4">Package Categories</h3>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
            {categories.map((category) => (
              <button
                key={category.id}
                onClick={() => setSelectedCategory(category.id)}
                className={`p-4 rounded-lg border-2 transition-colors ${
                  selectedCategory === category.id
                    ? 'border-blue-500 bg-blue-50'
                    : 'border-gray-200 hover:border-gray-300 hover:bg-gray-50'
                }`}
              >
                <div className="flex items-center justify-center mb-2">
                  {getCategoryIcon(category.id)}
                </div>
                <div className="text-sm font-medium text-gray-900">{category.name}</div>
                <div className="text-xs text-gray-500">{category.count} packages</div>
              </button>
            ))}
          </div>
        </div>

        {/* Packages List */}
        <div className="space-y-6">
          <div className="bg-white rounded-lg border border-gray-200">
            <div className="p-6 border-b border-gray-200">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-lg font-semibold text-gray-900">
                    {selectedCategory === 'all' ? 'All Packages' : categories.find(c => c.id === selectedCategory)?.name}
                  </h3>
                  <p className="text-sm text-gray-700 mt-1">{filteredPackages.length} packages in current release</p>
                </div>
              </div>
            </div>
            <div className="divide-y divide-gray-200">
              {filteredPackages.map((pkg) => (
                <div key={pkg.name} className="p-6 hover:bg-gray-50">
                  <div className="flex items-start justify-between">
                    <div className="flex items-start space-x-4 flex-1">
                      <div className="flex-shrink-0 mt-1">
                        {getStatusIcon(pkg.status)}
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center space-x-3 mb-2">
                          <h4 className="font-mono font-semibold text-gray-900">{pkg.name}</h4>
                          <span className="font-mono text-sm text-gray-600">{pkg.version}</span>
                          <span className={`inline-flex items-center px-2 py-1 text-xs font-medium rounded-full ${getCategoryColor(pkg.category)}`}>
                            {getCategoryIcon(pkg.category)}
                            <span className="ml-1">{pkg.category}</span>
                          </span>
                          <span className={`inline-flex px-2 py-1 text-xs font-medium rounded-full border ${getStatusColor(pkg.status)}`}>
                            {pkg.status}
                          </span>
                        </div>
                        
                        <p className="text-gray-700 mb-3">{pkg.description}</p>
                        
                        <div className="flex items-center space-x-6 text-sm text-gray-600 mb-2">
                          <div className="flex items-center space-x-1">
                            <HardDrive className="w-4 h-4" />
                            <span>{pkg.size}</span>
                          </div>
                          <span>Updated: {formatDate(pkg.lastUpdated)}</span>
                        </div>

                        {pkg.dependencies.length > 0 && (
                          <div className="mt-3">
                            <details>
                              <summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800 font-medium">
                                View dependencies ({pkg.dependencies.length})
                              </summary>
                              <div className="mt-2 pl-4 border-l-2 border-gray-200">
                                <div className="flex flex-wrap gap-2">
                                  {pkg.dependencies.map((dep, index) => (
                                    <span key={index} className="inline-flex px-2 py-1 text-xs bg-gray-100 text-gray-700 rounded font-mono">
                                      {dep}
                                    </span>
                                  ))}
                                </div>
                              </div>
                            </details>
                          </div>
                        )}
                      </div>
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

export default PackagesPage;