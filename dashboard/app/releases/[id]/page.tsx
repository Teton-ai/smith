'use client';

import React, { useState, useEffect } from 'react';
import { useRouter, useParams } from 'next/navigation';
import {
  Package,
  Tag,
  Download,
  Calendar,
  ArrowLeft,
  Box,
  Cpu,
  Monitor,
  HardDrive,
  Plus,
  Trash2,
  Eye,
  X,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";
import moment from 'moment';

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

interface Distribution {
  id: number;
  name: string;
  description: string | null;
  architecture: string;
  num_packages: number | null;
}

interface ReleasePackage {
  id: number;
  name: string;
  version: string;
  description?: string;
  size?: number;
  checksum?: string;
}

interface AvailablePackage {
  id: number;
  name: string;
  version: string;
  architecture: string;
  file: string;
  created_at: string;
}

const ReleaseDetailPage = () => {
  const router = useRouter();
  const params = useParams();
  const releaseId = params.id as string;
  const { callAPI, loading, error } = useSmithAPI();
  const [release, setRelease] = useState<Release | null>(null);
  const [distribution, setDistribution] = useState<Distribution | null>(null);
  const [packages, setPackages] = useState<ReleasePackage[]>([]);
  const [packagesLoading, setPackagesLoading] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [showAddPackageModal, setShowAddPackageModal] = useState(false);
  const [availablePackages, setAvailablePackages] = useState<AvailablePackage[]>([]);
  const [availablePackagesLoading, setAvailablePackagesLoading] = useState(false);
  const [selectedAvailablePackage, setSelectedAvailablePackage] = useState<number | null>(null);

  useEffect(() => {
    const fetchRelease = async () => {
      const data = await callAPI<Release>('GET', `/releases/${releaseId}`);
      if (data) {
        setRelease(data);
        
        // Fetch distribution info
        const distData = await callAPI<Distribution>('GET', `/distributions/${data.distribution_id}`);
        if (distData) {
          setDistribution(distData);
        }
      }
    };
    if (releaseId) {
      fetchRelease();
    }
  }, [releaseId, callAPI]);

  useEffect(() => {
    if (releaseId) {
      const fetchPackages = async () => {
        setPackagesLoading(true);
        try {
          const data = await callAPI<ReleasePackage[]>('GET', `/releases/${releaseId}/packages`);
          if (data) {
            setPackages(data);
          }
        } finally {
          setPackagesLoading(false);
        }
      };
      fetchPackages();
    }
  }, [releaseId, callAPI]);

  const formatRelativeTime = (dateString: string) => {
    return moment(dateString).fromNow();
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

  const handlePublishRelease = async () => {
    if (!release || !release.draft || publishing) return;
    
    setPublishing(true);
    try {
      const updatedRelease = await callAPI<Release>('PATCH', `/releases/${releaseId}`, {
        draft: false
      });
      
      if (updatedRelease) {
        setRelease(updatedRelease);
      }
    } catch (error: any) {
      console.error('Failed to publish release:', error);
      alert(`Failed to publish release: ${error?.message || 'Unknown error'}`);
    } finally {
      setPublishing(false);
    }
  };

  const fetchAvailablePackages = async () => {
    setAvailablePackagesLoading(true);
    try {
      const data = await callAPI<AvailablePackage[]>('GET', '/packages');
      if (data) {
        setAvailablePackages(data);
      }
    } catch (error: any) {
      console.error('Failed to fetch available packages:', error);
    } finally {
      setAvailablePackagesLoading(false);
    }
  };

  const handleAddPackage = async () => {
    if (!selectedAvailablePackage) return;
    
    try {
      await callAPI('POST', `/releases/${releaseId}/packages`, {
        id: selectedAvailablePackage
      });
      
      // Refresh packages list
      const data = await callAPI<ReleasePackage[]>('GET', `/releases/${releaseId}/packages`);
      if (data) {
        setPackages(data);
      }
      setSelectedAvailablePackage(null);
      setShowAddPackageModal(false);
    } catch (error: any) {
      console.error('Failed to add package:', error);
      alert(`Failed to add package: ${error?.message || 'Unknown error'}`);
    }
  };

  const handleDeletePackage = async (packageId: number) => {
    if (!confirm('Are you sure you want to delete this package?')) return;
    
    try {
      await callAPI('DELETE', `/releases/${releaseId}/packages/${packageId}`);
      setPackages(prev => prev.filter(pkg => pkg.id !== packageId));
    } catch (error: any) {
      console.error('Failed to delete package:', error);
      alert(`Failed to delete package: ${error?.message || 'Unknown error'}`);
    }
  };

  const openAddModal = async () => {
    setSelectedAvailablePackage(null);
    setShowAddPackageModal(true);
    await fetchAvailablePackages();
  };

  if (loading || !release) {
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
            onClick={() => distribution ? router.push(`/distributions/${distribution.id}`) : router.push('/distributions')}
            className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="text-sm font-medium">
              {distribution ? `Back to ${distribution.name}` : 'Back to Distributions'}
            </span>
          </button>
        </div>

        {/* Release Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-4">
          <div className="flex items-center space-x-3">
            <div className="p-2 bg-gray-100 text-gray-600 rounded">
              <Tag className="w-5 h-5" />
            </div>
            <div className="flex-1">
              <div className="flex items-center space-x-3">
                <h1 className="text-xl font-bold text-gray-900">Release {release.version}</h1>
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
              <div className="flex items-center space-x-4 mt-1 text-sm text-gray-600">
                <div className="flex items-center space-x-1">
                  <Calendar className="w-4 h-4" />
                  <span>Created {formatRelativeTime(release.created_at)}</span>
                </div>
                {distribution && (
                  <div className="flex items-center space-x-2">
                    <span className="font-medium">{distribution.name}</span>
                    <span className={`px-2 py-1 text-xs font-medium rounded-full ${getArchColor(distribution.architecture)}`}>
                      {distribution.architecture.toUpperCase()}
                    </span>
                  </div>
                )}
                {release.size && (
                  <span>{formatFileSize(release.size)}</span>
                )}
                {release.download_count !== undefined && (
                  <div className="flex items-center space-x-1">
                    <Download className="w-4 h-4" />
                    <span>{release.download_count} downloads</span>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Package Management Modals */}
        {showAddPackageModal && (
          <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
            <div className="bg-white rounded-lg shadow-xl p-6 w-[480px]">
              <div className="flex items-center justify-between mb-6">
                <h3 className="text-lg font-semibold text-gray-900">Add Package to Release</h3>
                <button
                  onClick={() => setShowAddPackageModal(false)}
                  className="text-gray-400 hover:text-gray-600"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>

              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Select Package *
                  </label>
                  {availablePackagesLoading ? (
                    <div className="flex items-center justify-center py-4">
                      <div className="text-gray-500 text-sm">Loading available packages...</div>
                    </div>
                  ) : (
                    <select
                      value={selectedAvailablePackage || ''}
                      onChange={(e) => setSelectedAvailablePackage(Number(e.target.value) || null)}
                      className="w-full px-3 py-2 border border-gray-300 rounded-md focus:ring-blue-500 focus:border-blue-500"
                    >
                      <option value="">Choose a package...</option>
                      {availablePackages
                        .filter(pkg => 
                          !packages.some(releasePkg => releasePkg.id === pkg.id) &&
                          (!distribution || pkg.architecture === distribution.architecture)
                        )
                        .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
                        .map((pkg) => (
                        <option key={pkg.id} value={pkg.id}>
                          {pkg.name} v{pkg.version} ({pkg.architecture})
                        </option>
                      ))}
                    </select>
                  )}
                  {selectedAvailablePackage && (
                    <div className="mt-3 p-3 bg-gray-50 rounded-md">
                      {(() => {
                        const pkg = availablePackages.find(p => p.id === selectedAvailablePackage);
                        return pkg ? (
                          <div className="text-sm">
                            <div className="font-medium text-gray-900">{pkg.name}</div>
                            <div className="text-gray-600 mt-1">
                              Version: {pkg.version} • Architecture: {pkg.architecture}
                            </div>
                            <div className="text-gray-500 mt-1">
                              File: {pkg.file}
                            </div>
                          </div>
                        ) : null;
                      })()}
                    </div>
                  )}
                </div>
              </div>

              <div className="flex justify-end space-x-3 mt-6">
                <button
                  onClick={() => setShowAddPackageModal(false)}
                  className="px-4 py-2 text-sm font-medium text-gray-600 hover:text-gray-800 hover:bg-gray-100 rounded-md transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={handleAddPackage}
                  disabled={!selectedAvailablePackage}
                  className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                    !selectedAvailablePackage
                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                      : 'bg-blue-600 text-white hover:bg-blue-700'
                  }`}
                >
                  Add Package
                </button>
              </div>
            </div>
          </div>
        )}


        {/* Packages Section */}
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2">
              <Box className="w-5 h-5 text-gray-600" />
              <h2 className="text-lg font-semibold text-gray-900">Packages</h2>
              <span className="text-sm text-gray-500">({packages.length})</span>
            </div>
            {release?.draft && (
              <div className="flex items-center space-x-3">
                <button
                  onClick={handlePublishRelease}
                  disabled={publishing}
                  className={`flex items-center space-x-2 px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                    publishing
                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                      : 'bg-green-600 text-white hover:bg-green-700'
                  }`}
                >
                  {publishing ? (
                    <>
                      <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
                      <span>Publishing...</span>
                    </>
                  ) : (
                    <>
                      <Eye className="w-4 h-4" />
                      <span>Publish Release</span>
                    </>
                  )}
                </button>
                <button
                  onClick={openAddModal}
                  className="flex items-center space-x-2 px-4 py-2 bg-blue-600 text-white text-sm font-medium rounded-md hover:bg-blue-700 transition-colors"
                >
                  <Plus className="w-4 h-4" />
                  <span>Add Package</span>
                </button>
              </div>
            )}
          </div>
          
          <div className="bg-white rounded border border-gray-200 overflow-hidden">
            {packagesLoading ? (
              <div className="p-6 text-center">
                <div className="text-gray-500 text-sm">Loading packages...</div>
              </div>
            ) : packages.length === 0 ? (
              <div className="p-6 text-center">
                <Box className="w-8 h-8 text-gray-400 mx-auto mb-2" />
                <p className="text-sm text-gray-500">No packages found for this release</p>
              </div>
            ) : (
              <div className="divide-y divide-gray-200">
                {packages.map((pkg) => (
                  <div key={pkg.id} className="p-4 hover:bg-gray-50 transition-colors">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center space-x-3">
                        <div className="p-2 bg-gray-100 text-gray-600 rounded">
                          <Package className="w-4 h-4" />
                        </div>
                        <div>
                          <div className="flex items-center space-x-2">
                            <h4 className="font-medium text-gray-900">{pkg.name}</h4>
                            <span className="text-xs text-gray-500">v{pkg.version}</span>
                          </div>
                          {pkg.description && (
                            <p className="mt-1 text-sm text-gray-600">{pkg.description}</p>
                          )}
                          <div className="flex items-center space-x-3 mt-1 text-xs text-gray-500">
                            {pkg.size && (
                              <span>{formatFileSize(pkg.size)}</span>
                            )}
                            {pkg.checksum && (
                              <span className="font-mono">{pkg.checksum.substring(0, 16)}...</span>
                            )}
                          </div>
                        </div>
                      </div>
                      {release?.draft && (
                        <div className="flex items-center space-x-2">
                          <button
                            onClick={() => handleDeletePackage(pkg.id)}
                            className="p-2 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded-md transition-colors"
                            title="Remove package from release"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default ReleaseDetailPage;