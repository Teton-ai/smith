'use client';

import React, { useState, useEffect } from 'react';
import { useParams, useRouter } from 'next/navigation';
import {
  Cpu,
  ChevronRight,
  Package,
  Download,
  CheckCircle,
  Clock,
  AlertTriangle,
  ArrowUp,
  Calendar,
  GitBranch,
  Zap,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";

// Mock releases data
const mockReleases = [
  {
    id: 398,
    version: "2.35.0",
    name: "Stable Release 2.35.0",
    description: "Security updates, bug fixes, and performance improvements",
    status: "deployed",
    deployedAt: "2025-01-13T14:04:19Z",
    createdAt: "2025-01-10T09:30:00Z",
    size: "245 MB",
    packages: [
      { name: "smith-agent", version: "2.35.0", type: "core" },
      { name: "linux-tegra", version: "5.10.104-tegra", type: "kernel" },
      { name: "nvidia-l4t-core", version: "35.4.1", type: "system" }
    ],
    changelog: [
      "Fixed memory leak in telemetry collection",
      "Enhanced network connectivity reliability", 
      "Updated security certificates",
      "Improved error handling for edge cases"
    ]
  },
  {
    id: 387,
    version: "2.34.2",
    name: "Hotfix Release 2.34.2", 
    description: "Critical security patch",
    status: "superseded",
    deployedAt: "2025-01-05T16:22:00Z",
    createdAt: "2025-01-04T14:15:00Z",
    size: "198 MB",
    packages: [
      { name: "smith-agent", version: "2.34.2", type: "core" },
      { name: "linux-tegra", version: "5.10.104-tegra", type: "kernel" }
    ],
    changelog: [
      "Security patch for CVE-2024-12345",
      "Fixed authentication timeout issue"
    ]
  },
  {
    id: 375,
    version: "2.34.0",
    name: "Feature Release 2.34.0",
    description: "New features and improvements",
    status: "superseded", 
    deployedAt: "2024-12-20T10:15:00Z",
    createdAt: "2024-12-18T08:00:00Z",
    size: "267 MB",
    packages: [
      { name: "smith-agent", version: "2.34.0", type: "core" },
      { name: "linux-tegra", version: "5.10.104-tegra", type: "kernel" },
      { name: "nvidia-l4t-core", version: "35.4.0", type: "system" }
    ],
    changelog: [
      "Added new telemetry metrics collection",
      "Improved power management",
      "Enhanced logging system",
      "UI/UX improvements in local interface"
    ]
  }
];

const mockAvailableReleases = [
  {
    id: 410,
    version: "2.36.0",
    name: "Beta Release 2.36.0",
    description: "Latest features and improvements (Beta)",
    createdAt: "2025-01-15T11:00:00Z",
    size: "268 MB",
    isRecommended: false,
    isBeta: true,
    changelog: [
      "Experimental AI-powered diagnostics",
      "New dashboard widgets",
      "Enhanced API endpoints"
    ]
  },
  {
    id: 405,
    version: "2.35.1",
    name: "Stable Release 2.35.1", 
    description: "Latest stable release with bug fixes",
    createdAt: "2025-01-14T15:30:00Z",
    size: "247 MB",
    isRecommended: true,
    isBeta: false,
    changelog: [
      "Fixed edge case in network reconnection",
      "Improved startup performance",
      "Minor UI enhancements"
    ]
  }
];

const ReleasesPage = () => {
  const params = useParams();
  const router = useRouter();
  const [releases, setReleases] = useState(mockReleases);
  const [availableReleases, setAvailableReleases] = useState(mockAvailableReleases);
  const [loading, setLoading] = useState(false);

  const serial = params.serial as string;

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short', 
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    });
  };

  const getStatusIcon = (status: string) => {
    switch (status) {
      case 'deployed':
        return <CheckCircle className="w-4 h-4 text-green-500" />;
      case 'superseded':
        return <Clock className="w-4 h-4 text-yellow-500" />;
      default:
        return <AlertTriangle className="w-4 h-4 text-gray-500" />;
    }
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'deployed':
        return 'text-green-600 bg-green-50 border-green-200';
      case 'superseded':
        return 'text-yellow-600 bg-yellow-50 border-yellow-200';
      default:
        return 'text-gray-600 bg-gray-50 border-gray-200';
    }
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
          <span className="text-gray-900 font-medium">Releases</span>
        </div>

        {/* Device Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center space-x-4">
            <div className="p-3 bg-gray-100 rounded-lg">
              <Package className="w-8 h-8 text-gray-600" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-gray-900">{serial}</h1>
              <p className="text-gray-600 mt-1">Release History & Information</p>
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

        {/* Available Updates */}
        {availableReleases.length > 0 && (
          <div className="bg-blue-50 border border-blue-200 rounded-lg p-6">
            <div className="flex items-start space-x-3">
              <ArrowUp className="w-5 h-5 text-blue-500 mt-0.5" />
              <div className="flex-1">
                <h3 className="text-lg font-semibold text-blue-900 mb-3">Available Releases</h3>
                <div className="space-y-3">
                  {availableReleases.map((release) => (
                    <div key={release.id} className="bg-white rounded-lg border border-blue-200 p-4">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center space-x-3">
                          <div className="flex items-center space-x-2">
                            <GitBranch className="w-4 h-4 text-gray-500" />
                            <span className="font-mono text-sm font-semibold text-gray-900">{release.version}</span>
                            {release.isRecommended && (
                              <span className="inline-flex px-2 py-1 text-xs font-medium bg-green-100 text-green-800 rounded-full">
                                Recommended
                              </span>
                            )}
                            {release.isBeta && (
                              <span className="inline-flex px-2 py-1 text-xs font-medium bg-orange-100 text-orange-800 rounded-full">
                                Beta
                              </span>
                            )}
                          </div>
                        </div>
                      </div>
                      <p className="text-gray-700 mt-2">{release.description}</p>
                      <div className="flex items-center space-x-4 mt-2 text-sm text-gray-500">
                        <span>Size: {release.size}</span>
                        <span>Released: {formatDate(release.createdAt)}</span>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Release History */}
        <div className="space-y-6">
          <div className="bg-white rounded-lg border border-gray-200">
            <div className="p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">Release History</h3>
              <p className="text-sm text-gray-700 mt-1">Previous releases deployed to this device</p>
            </div>
            <div className="divide-y divide-gray-200">
              {releases.map((release) => (
                <div key={release.id} className="p-6 hover:bg-gray-50">
                  <div className="flex items-start justify-between">
                    <div className="flex items-start space-x-4 flex-1">
                      <div className="flex-shrink-0 mt-1">
                        {getStatusIcon(release.status)}
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center space-x-3 mb-2">
                          <div className="flex items-center space-x-2">
                            <GitBranch className="w-4 h-4 text-gray-500" />
                            <span className="font-mono text-sm font-semibold text-gray-900">{release.version}</span>
                          </div>
                          <span className={`inline-flex px-2 py-1 text-xs font-medium rounded-full border ${getStatusColor(release.status)}`}>
                            {release.status}
                          </span>
                        </div>
                        
                        <h4 className="font-medium text-gray-900 mb-1">{release.name}</h4>
                        <p className="text-gray-700 mb-3">{release.description}</p>
                        
                        <div className="flex items-center space-x-6 text-sm text-gray-600 mb-3">
                          <div className="flex items-center space-x-1">
                            <Calendar className="w-4 h-4" />
                            <span>Deployed: {formatDate(release.deployedAt)}</span>
                          </div>
                          <div className="flex items-center space-x-1">
                            <Package className="w-4 h-4" />
                            <span>Size: {release.size}</span>
                          </div>
                          <div className="flex items-center space-x-1">
                            <Zap className="w-4 h-4" />
                            <span>{release.packages.length} packages</span>
                          </div>
                        </div>

                        <details className="mt-3">
                          <summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800 font-medium">
                            View changelog
                          </summary>
                          <div className="mt-3 pl-4 border-l-2 border-gray-200">
                            <ul className="space-y-1 text-sm text-gray-700">
                              {release.changelog.map((change, index) => (
                                <li key={index} className="flex items-start">
                                  <span className="text-gray-400 mr-2">•</span>
                                  {change}
                                </li>
                              ))}
                            </ul>
                          </div>
                        </details>
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

export default ReleasesPage;