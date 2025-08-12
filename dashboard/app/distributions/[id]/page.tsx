'use client';

import React, { useState, useEffect } from 'react';
import { useRouter, useParams } from 'next/navigation';
import {
  Package,
  Layers,
  Monitor,
  HardDrive,
  Cpu,
  Download,
  Calendar,
  Tag,
  BarChart3,
  ArrowLeft,
  Users,
  Activity,
  ChevronRight,
  Clock,
  CheckCircle,
  Computer,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";

interface Distribution {
  id: number;
  name: string;
  description: string | null;
  architecture: string;
  num_packages: number | null;
}

interface Rollout {
  distribution_id: number;
  pending_devices: number | null;
  total_devices: number | null;
  updated_devices: number | null;
}

interface Release {
  id: number;
  version: string;
  distribution_id: number;
  created_at: string;
  size?: number;
  download_count?: number;
  yanked?: boolean;
  draft?: boolean;
}

type TabType = 'overview' | 'releases';

const DistributionDetailPage = () => {
  const router = useRouter();
  const params = useParams();
  const distributionId = params.id as string;
  const { callAPI, loading, error } = useSmithAPI();
  const [distribution, setDistribution] = useState<Distribution | null>(null);
  const [rollout, setRollout] = useState<Rollout | null>(null);
  const [releases, setReleases] = useState<Release[]>([]);
  const [activeTab, setActiveTab] = useState<TabType>('overview');
  const [releasesLoading, setReleasesLoading] = useState(false);

  useEffect(() => {
    const fetchDistribution = async () => {
      const data = await callAPI<Distribution>('GET', `/distributions/${distributionId}`);
      if (data) {
        setDistribution(data);
      }
    };

    const fetchRollout = async () => {
      const data = await callAPI<Rollout>('GET', `/distributions/${distributionId}/rollout`);
      if (data) {
        setRollout(data);
      }
    };

    if (distributionId) {
      fetchDistribution();
      fetchRollout();
    }
  }, [distributionId, callAPI]);

  useEffect(() => {
    if (activeTab === 'releases' && distributionId) {
      const fetchReleases = async () => {
        setReleasesLoading(true);
        try {
          const data = await callAPI<Release[]>('GET', `/distributions/${distributionId}/releases`);
          if (data) {
            setReleases(data);
          }
        } finally {
          setReleasesLoading(false);
        }
      };
      fetchReleases();
    }
  }, [activeTab, distributionId, callAPI]);

  const getArchIcon = (architecture: string) => {
    switch (architecture.toLowerCase()) {
      case 'x86_64':
      case 'amd64':
        return <Monitor className="w-5 h-5" />;
      case 'arm64':
      case 'aarch64':
        return <Cpu className="w-5 h-5" />;
      case 'armv7':
      case 'arm':
        return <HardDrive className="w-5 h-5" />;
      default:
        return <Package className="w-5 h-5" />;
    }
  };

  const getArchColor = (architecture: string) => {
    switch (architecture.toLowerCase()) {
      case 'x86_64':
      case 'amd64':
        return 'bg-blue-100 text-blue-700';
      case 'arm64':
      case 'aarch64':
        return 'bg-green-100 text-green-700';
      case 'armv7':
      case 'arm':
        return 'bg-purple-100 text-purple-700';
      default:
        return 'bg-gray-100 text-gray-700';
    }
  };

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const formatFileSize = (bytes?: number) => {
    if (!bytes) return 'Unknown';
    const mb = bytes / (1024 * 1024);
    if (mb < 1024) {
      return `${mb.toFixed(1)} MB`;
    }
    const gb = mb / 1024;
    return `${gb.toFixed(1)} GB`;
  };

  if (loading || !distribution) {
    return (
      <PrivateLayout id="distributions">
        <div className="flex items-center justify-center h-32">
          <div className="text-gray-500 text-sm">Loading...</div>
        </div>
      </PrivateLayout>
    );
  }

  if (error) {
    return (
      <PrivateLayout id="distributions">
        <div className="flex items-center justify-center h-32">
          <div className="text-red-500 text-sm">Error: {error}</div>
        </div>
      </PrivateLayout>
    );
  }

  return (
    <PrivateLayout id="distributions">
      <div className="space-y-6">
        {/* Header with Back Button */}
        <div className="flex items-center space-x-4">
          <button
            onClick={() => router.push('/distributions')}
            className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="text-sm font-medium">Back to Distributions</span>
          </button>
        </div>

        {/* Distribution Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-6">
          <div className="flex items-center space-x-4">
            <div className={`p-3 rounded-lg ${getArchColor(distribution.architecture)}`}>
              {getArchIcon(distribution.architecture)}
            </div>
            <div className="flex-1">
              <div className="flex items-center space-x-3">
                <h1 className="text-2xl font-bold text-gray-900">{distribution.name}</h1>
                <span className="text-sm text-gray-500">#{distribution.id}</span>
              </div>
              <div className="flex items-center space-x-6 mt-2 text-sm text-gray-600">
                <span className="font-medium">{distribution.architecture}</span>
                <span>{distribution.num_packages || 0} packages</span>
              </div>
              {distribution.description && (
                <p className="mt-2 text-sm text-gray-600">{distribution.description}</p>
              )}
            </div>
          </div>
        </div>

        {/* Tab Navigation */}
        <div className="border-b border-gray-200">
          <nav className="-mb-px flex space-x-8">
            <button
              onClick={() => setActiveTab('overview')}
              className={`py-2 px-1 border-b-2 font-medium text-sm ${
                activeTab === 'overview'
                  ? 'border-blue-500 text-blue-600'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
              }`}
            >
              <div className="flex items-center space-x-2">
                <BarChart3 className="w-4 h-4" />
                <span>Overview</span>
              </div>
            </button>
            <button
              onClick={() => setActiveTab('releases')}
              className={`py-2 px-1 border-b-2 font-medium text-sm ${
                activeTab === 'releases'
                  ? 'border-blue-500 text-blue-600'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
              }`}
            >
              <div className="flex items-center space-x-2">
                <Tag className="w-4 h-4" />
                <span>Releases</span>
              </div>
            </button>
          </nav>
        </div>

        {/* Tab Content */}
        {activeTab === 'overview' && (
          <div className="space-y-6">
            {/* Overview Stats */}
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6 gap-6">
              <div className="bg-white rounded-lg border border-gray-200 p-6">
                <div className="flex items-center">
                  <Package className="w-8 h-8 text-blue-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Packages</p>
                    <p className="text-2xl font-bold text-gray-900">{distribution.num_packages || 0}</p>
                  </div>
                </div>
              </div>

              <div className="bg-white rounded-lg border border-gray-200 p-6">
                <div className="flex items-center">
                  <Tag className="w-8 h-8 text-green-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Total Releases</p>
                    <p className="text-2xl font-bold text-gray-900">{releases.length}</p>
                  </div>
                </div>
              </div>

              <div className="bg-white rounded-lg border border-gray-200 p-6">
                <div className="flex items-center">
                  <Activity className="w-8 h-8 text-purple-500" />
                  <div className="ml-4">
                    <p className="text-sm font-medium text-gray-600">Architecture</p>
                    <p className="text-lg font-semibold text-gray-900">{distribution.architecture}</p>
                  </div>
                </div>
              </div>

              {rollout && (
                <>
                  <div className="bg-white rounded-lg border border-gray-200 p-6">
                    <div className="flex items-center">
                      <Computer className="w-8 h-8 text-indigo-500" />
                      <div className="ml-4">
                        <p className="text-sm font-medium text-gray-600">Total Devices</p>
                        <p className="text-2xl font-bold text-gray-900">{rollout.total_devices ?? 'N/A'}</p>
                      </div>
                    </div>
                  </div>

                  <div className="bg-white rounded-lg border border-gray-200 p-6">
                    <div className="flex items-center">
                      <CheckCircle className="w-8 h-8 text-green-500" />
                      <div className="ml-4">
                        <p className="text-sm font-medium text-gray-600">Updated Devices</p>
                        <p className="text-2xl font-bold text-gray-900">{rollout.updated_devices ?? 'N/A'}</p>
                      </div>
                    </div>
                  </div>

                  <div className="bg-white rounded-lg border border-gray-200 p-6">
                    <div className="flex items-center">
                      <Clock className="w-8 h-8 text-orange-500" />
                      <div className="ml-4">
                        <p className="text-sm font-medium text-gray-600">Pending Devices</p>
                        <p className="text-2xl font-bold text-gray-900">{rollout.pending_devices ?? 'N/A'}</p>
                      </div>
                    </div>
                  </div>
                </>
              )}
            </div>

            {/* Distribution Details */}
            <div className="bg-white rounded-lg border border-gray-200 p-6">
              <h3 className="text-lg font-semibold text-gray-900 mb-4">Distribution Details</h3>
              <dl className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <dt className="text-sm font-medium text-gray-500">ID</dt>
                  <dd className="mt-1 text-sm text-gray-900">#{distribution.id}</dd>
                </div>
                <div>
                  <dt className="text-sm font-medium text-gray-500">Name</dt>
                  <dd className="mt-1 text-sm text-gray-900">{distribution.name}</dd>
                </div>
                <div>
                  <dt className="text-sm font-medium text-gray-500">Architecture</dt>
                  <dd className="mt-1 text-sm text-gray-900">{distribution.architecture}</dd>
                </div>
                <div>
                  <dt className="text-sm font-medium text-gray-500">Package Count</dt>
                  <dd className="mt-1 text-sm text-gray-900">{distribution.num_packages || 'Unknown'}</dd>
                </div>
                {distribution.description && (
                  <div className="md:col-span-2">
                    <dt className="text-sm font-medium text-gray-500">Description</dt>
                    <dd className="mt-1 text-sm text-gray-900">{distribution.description}</dd>
                  </div>
                )}
              </dl>
            </div>
          </div>
        )}

        {activeTab === 'releases' && (
          <div className="space-y-6">
            {/* Releases List */}
            <div className="bg-white rounded border border-gray-200 overflow-hidden">
              {releasesLoading ? (
                <div className="p-6 text-center">
                  <div className="text-gray-500 text-sm">Loading releases...</div>
                </div>
              ) : releases.length === 0 ? (
                <div className="p-6 text-center">
                  <Tag className="w-8 h-8 text-gray-400 mx-auto mb-2" />
                  <p className="text-sm text-gray-500">No releases found for this distribution</p>
                </div>
              ) : (
                <div className="divide-y divide-gray-200">
                  {releases.map((release) => (
                    <div 
                      key={release.id} 
                      className="p-4 hover:bg-gray-50 cursor-pointer transition-colors"
                      onClick={() => router.push(`/releases/${release.id}`)}
                    >
                      <div className="flex items-center justify-between">
                        <div className="flex items-center space-x-3">
                          <div className="p-2 bg-green-100 text-green-700 rounded">
                            <Tag className="w-4 h-4" />
                          </div>
                          <div>
                            <div className="flex items-center space-x-2">
                              <h4 className="font-medium text-gray-900">{release.version}</h4>
                              <span className="text-xs text-gray-500">#{release.id}</span>
                              {release.draft && (
                                <span className="px-2 py-1 text-xs font-medium bg-yellow-100 text-yellow-800 rounded-full">
                                  Draft
                                </span>
                              )}
                              {release.yanked && (
                                <span className="px-2 py-1 text-xs font-medium bg-red-100 text-red-800 rounded-full">
                                  Yanked
                                </span>
                              )}
                            </div>
                            <div className="flex items-center space-x-4 mt-1 text-xs text-gray-500">
                              <div className="flex items-center space-x-1">
                                <Calendar className="w-3 h-3" />
                                <span>{formatDate(release.created_at)}</span>
                              </div>
                              {release.size && (
                                <span>{formatFileSize(release.size)}</span>
                              )}
                            </div>
                          </div>
                        </div>
                        <div className="flex items-center space-x-4">
                          {release.download_count !== undefined && (
                            <div className="flex items-center space-x-1 text-xs text-gray-500">
                              <Download className="w-3 h-3" />
                              <span>{release.download_count} downloads</span>
                            </div>
                          )}
                          <ChevronRight className="w-4 h-4 text-gray-400" />
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </PrivateLayout>
  );
};

export default DistributionDetailPage;