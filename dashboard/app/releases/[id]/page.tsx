'use client';

import React, { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
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
  Rocket,
  Search,
  RefreshCw,
  ArrowUp,
  CheckCircle,
  XCircle,
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
  const [showReplacePackageModal, setShowReplacePackageModal] = useState(false);
  const [packageToReplace, setPackageToReplace] = useState<ReleasePackage | null>(null);
  const [availablePackages, setAvailablePackages] = useState<AvailablePackage[]>([]);
  const [availablePackagesLoading, setAvailablePackagesLoading] = useState(false);
  const [selectedAvailablePackage, setSelectedAvailablePackage] = useState<number | null>(null);
  const [packageSearchQuery, setPackageSearchQuery] = useState('');
  const [showDeployModal, setShowDeployModal] = useState(false);
  const [deploying, setDeploying] = useState(false);
  const [mounted, setMounted] = useState(false);
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null);
  const [upgradingPackages, setUpgradingPackages] = useState<Set<number>>(new Set());

  useEffect(() => {
    setMounted(true);
  }, []);

  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [toast]);


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

  useEffect(() => {
    if (releaseId) {
      fetchAvailablePackages();
    }
  }, [releaseId]);

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
      await callAPI('POST', `/releases/${releaseId}`, {
        draft: false
      });

      // Refresh the release to get updated state
      const updatedRelease = await callAPI<Release>('GET', `/releases/${releaseId}`);
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
    setPackageSearchQuery('');
    setShowAddPackageModal(true);
    // Only fetch if we don't have packages cached
    if (availablePackages.length === 0) {
      await fetchAvailablePackages();
    }
  };

  const openReplaceModal = async (pkg: ReleasePackage) => {
    setPackageToReplace(pkg);
    setSelectedAvailablePackage(null);
    setPackageSearchQuery('');
    setShowReplacePackageModal(true);
    // Only fetch if we don't have packages cached
    if (availablePackages.length === 0) {
      await fetchAvailablePackages();
    }
  };

  const handleReplacePackage = async () => {
    if (!selectedAvailablePackage || !packageToReplace) return;

    try {
      // Delete old package
      await callAPI('DELETE', `/releases/${releaseId}/packages/${packageToReplace.id}`);

      // Add new package
      await callAPI('POST', `/releases/${releaseId}/packages`, {
        id: selectedAvailablePackage
      });

      // Refresh packages list
      const data = await callAPI<ReleasePackage[]>('GET', `/releases/${releaseId}/packages`);
      if (data) {
        setPackages(data);
      }
      setSelectedAvailablePackage(null);
      setPackageToReplace(null);
      setShowReplacePackageModal(false);
    } catch (error: any) {
      console.error('Failed to replace package:', error);
      alert(`Failed to replace package: ${error?.message || 'Unknown error'}`);
    }
  };

  const compareVersions = (v1: string, v2: string): number => {
    const parts1 = v1.replace(/^v/, '').split('.').map(Number);
    const parts2 = v2.replace(/^v/, '').split('.').map(Number);

    for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
      const part1 = parts1[i] || 0;
      const part2 = parts2[i] || 0;
      if (part1 > part2) return 1;
      if (part1 < part2) return -1;
    }
    return 0;
  };

  const getLatestVersionForPackage = (pkg: ReleasePackage) => {
    if (availablePackages.length === 0) return null;

    const sameNamePackages = availablePackages.filter(
      availPkg =>
        availPkg.name === pkg.name &&
        availPkg.id !== pkg.id &&
        (!distribution || availPkg.architecture === distribution.architecture) &&
        compareVersions(availPkg.version, pkg.version) > 0 // Only show if version is actually higher
    );

    if (sameNamePackages.length === 0) return null;

    // Sort by semantic version to get the actual latest
    const sorted = [...sameNamePackages].sort(
      (a, b) => compareVersions(b.version, a.version)
    );

    return sorted[0];
  };

  const handleUpgradePackage = async (pkg: ReleasePackage) => {
    const latestVersion = getLatestVersionForPackage(pkg);
    if (!latestVersion) return;

    setUpgradingPackages(prev => new Set(prev).add(pkg.id));

    try {
      // Delete old package
      await callAPI('DELETE', `/releases/${releaseId}/packages/${pkg.id}`);

      // Add new package
      await callAPI('POST', `/releases/${releaseId}/packages`, {
        id: latestVersion.id
      });

      // Refresh packages list
      const data = await callAPI<ReleasePackage[]>('GET', `/releases/${releaseId}/packages`);
      if (data) {
        setPackages(data);
      }

      setToast({
        message: `Upgraded ${pkg.name} to v${latestVersion.version}`,
        type: 'success'
      });
    } catch (error: any) {
      console.error('Failed to upgrade package:', error);
      setToast({
        message: `Failed to upgrade ${pkg.name}: ${error?.message || 'Unknown error'}`,
        type: 'error'
      });
    } finally {
      setUpgradingPackages(prev => {
        const newSet = new Set(prev);
        newSet.delete(pkg.id);
        return newSet;
      });
    }
  };

  const handleDeployRelease = async () => {
    if (!release || deploying) return;

    setDeploying(true);
    try {
      await callAPI('POST', `/releases/${releaseId}/deployment`);
      setShowDeployModal(false);
      router.push(`/releases/${releaseId}/deployment`);
    } catch (error: any) {
      console.error('Failed to deploy release:', error);
      alert(`Failed to deploy release: ${error?.message || 'Unknown error'}`);
    } finally {
      setDeploying(false);
    }
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
      {/* Toast Notification */}
      {mounted && toast && createPortal(
        <div className="fixed top-4 right-4 z-50 animate-in slide-in-from-top-5 duration-300">
          <div className={`flex items-center space-x-3 px-4 py-3 rounded-lg shadow-lg ${
            toast.type === 'success' ? 'bg-green-600' : 'bg-red-600'
          }`}>
            {toast.type === 'success' ? (
              <CheckCircle className="w-5 h-5 text-white" />
            ) : (
              <XCircle className="w-5 h-5 text-white" />
            )}
            <span className="text-white font-medium text-sm">{toast.message}</span>
          </div>
        </div>,
        document.body
      )}

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

        {/* Deploy Confirmation Modal */}
        {mounted && showDeployModal && createPortal(
          <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
            <div className="bg-white rounded-lg shadow-xl p-6 w-[520px] animate-in zoom-in-95 duration-200">
              <div className="flex items-center justify-between mb-6">
                <h3 className="text-lg font-semibold text-gray-900">Deploy Release v{release.version}</h3>
                <button
                  onClick={() => setShowDeployModal(false)}
                  className="text-gray-400 hover:text-gray-600 cursor-pointer"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>

              <div className="space-y-4 mb-6">
                <p className="text-sm text-gray-700">
                  This will deploy the release in two phases:
                </p>

                <div className="bg-blue-50 border border-blue-200 rounded-lg p-4">
                  <h4 className="font-medium text-blue-900 text-sm mb-2">Phase 1: Canary Deployment</h4>
                  <ul className="text-sm text-blue-800 space-y-1 list-disc list-inside">
                    <li>Deploy to ~10 recently active devices</li>
                    <li>Wait for successful updates (up to 5 minutes)</li>
                    <li>You will be prompted to confirm when complete</li>
                  </ul>
                </div>

                <div className="bg-green-50 border border-green-200 rounded-lg p-4">
                  <h4 className="font-medium text-green-900 text-sm mb-2">Phase 2: Full Rollout</h4>
                  <ul className="text-sm text-green-800 space-y-1 list-disc list-inside">
                    <li>Manually confirm to proceed after canary success</li>
                    <li>Deploy to all remaining devices in distribution</li>
                    <li>Devices will update as they come online</li>
                  </ul>
                </div>
              </div>

              <div className="flex justify-end space-x-3">
                <button
                  onClick={() => setShowDeployModal(false)}
                  disabled={deploying}
                  className="px-4 py-2 text-sm font-medium text-gray-600 hover:text-gray-800 hover:bg-gray-100 rounded-md transition-colors cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  onClick={handleDeployRelease}
                  disabled={deploying}
                  className={`flex items-center space-x-2 px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                    deploying
                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                      : 'bg-blue-600 text-white hover:bg-blue-700 cursor-pointer'
                  }`}
                >
                  {deploying ? (
                    <>
                      <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
                      <span>Starting...</span>
                    </>
                  ) : (
                    <>
                      <Rocket className="w-4 h-4" />
                      <span>Start Deployment</span>
                    </>
                  )}
                </button>
              </div>
            </div>
          </div>,
          document.body
        )}

        {/* Replace Package Modal */}
        {mounted && showReplacePackageModal && packageToReplace && createPortal(
          <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
            <div className="bg-white rounded-lg shadow-xl p-6 w-[640px] animate-in zoom-in-95 duration-200">
              <div className="flex items-center justify-between mb-6">
                <h3 className="text-lg font-semibold text-gray-900">Replace Package: {packageToReplace.name}</h3>
                <button
                  onClick={() => setShowReplacePackageModal(false)}
                  className="text-gray-400 hover:text-gray-600 cursor-pointer"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>

              <div className="mb-4 p-3 bg-gray-50 rounded-md">
                <div className="text-sm">
                  <div className="font-medium text-gray-900">Current Version</div>
                  <div className="text-gray-600 mt-1">
                    {packageToReplace.name} v{packageToReplace.version}
                  </div>
                </div>
              </div>

              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Search Versions
                  </label>
                  <div className="relative">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
                    <input
                      type="text"
                      placeholder="Search by version..."
                      value={packageSearchQuery}
                      onChange={(e) => setPackageSearchQuery(e.target.value)}
                      className="w-full pl-10 pr-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-blue-500 focus:border-blue-500 placeholder:text-gray-400"
                    />
                  </div>
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Available Versions
                  </label>
                  {availablePackagesLoading ? (
                    <div className="flex items-center justify-center py-8">
                      <div className="text-gray-500 text-sm">Loading available versions...</div>
                    </div>
                  ) : (() => {
                    const filteredPackages = availablePackages
                      .filter(pkg =>
                        pkg.name === packageToReplace.name &&
                        pkg.id !== packageToReplace.id &&
                        (!distribution || pkg.architecture === distribution.architecture) &&
                        (packageSearchQuery === '' ||
                          pkg.version.toLowerCase().includes(packageSearchQuery.toLowerCase()))
                      )
                      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());

                    return filteredPackages.length === 0 ? (
                      <div className="text-center py-8 text-gray-500 text-sm">
                        {packageSearchQuery ? 'No versions match your search' : 'No other versions available'}
                      </div>
                    ) : (
                      <div className="border border-gray-200 rounded-md max-h-[320px] overflow-y-auto">
                        {filteredPackages.map((pkg) => (
                          <button
                            key={pkg.id}
                            onClick={() => setSelectedAvailablePackage(pkg.id)}
                            className={`w-full text-left p-3 border-b border-gray-200 last:border-b-0 hover:bg-gray-50 transition-colors cursor-pointer ${
                              selectedAvailablePackage === pkg.id ? 'bg-blue-50 hover:bg-blue-50' : ''
                            }`}
                          >
                            <div className="flex items-start justify-between">
                              <div className="flex-1 min-w-0">
                                <div className="flex items-center space-x-2">
                                  <span className="font-medium text-gray-900">{pkg.name}</span>
                                  <span className="text-xs text-gray-500">v{pkg.version}</span>
                                  <span className={`px-2 py-0.5 text-xs font-medium rounded ${getArchColor(pkg.architecture)}`}>
                                    {pkg.architecture}
                                  </span>
                                </div>
                                <div className="text-xs text-gray-500 mt-1 truncate">
                                  {pkg.file}
                                </div>
                              </div>
                              {selectedAvailablePackage === pkg.id && (
                                <div className="flex-shrink-0 ml-2">
                                  <div className="w-5 h-5 bg-blue-600 rounded-full flex items-center justify-center">
                                    <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                                    </svg>
                                  </div>
                                </div>
                              )}
                            </div>
                          </button>
                        ))}
                      </div>
                    );
                  })()}
                </div>
              </div>

              <div className="flex justify-end space-x-3 mt-6">
                <button
                  onClick={() => setShowReplacePackageModal(false)}
                  className="px-4 py-2 text-sm font-medium text-gray-600 hover:text-gray-800 hover:bg-gray-100 rounded-md transition-colors cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  onClick={handleReplacePackage}
                  disabled={!selectedAvailablePackage}
                  className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                    !selectedAvailablePackage
                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                      : 'bg-blue-600 text-white hover:bg-blue-700 cursor-pointer'
                  }`}
                >
                  Replace Version
                </button>
              </div>
            </div>
          </div>,
          document.body
        )}

        {/* Package Management Modals */}
        {mounted && showAddPackageModal && createPortal(
          <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 animate-in fade-in duration-200">
            <div className="bg-white rounded-lg shadow-xl p-6 w-[640px] animate-in zoom-in-95 duration-200">
              <div className="flex items-center justify-between mb-6">
                <h3 className="text-lg font-semibold text-gray-900">Add Package to Release</h3>
                <button
                  onClick={() => setShowAddPackageModal(false)}
                  className="text-gray-400 hover:text-gray-600 cursor-pointer"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>

              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Search Packages
                  </label>
                  <div className="relative">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
                    <input
                      type="text"
                      placeholder="Search by name or version..."
                      value={packageSearchQuery}
                      onChange={(e) => setPackageSearchQuery(e.target.value)}
                      className="w-full pl-10 pr-3 py-2 bg-white text-gray-900 border border-gray-300 rounded-md focus:ring-blue-500 focus:border-blue-500 placeholder:text-gray-400"
                    />
                  </div>
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Available Packages
                  </label>
                  {availablePackagesLoading ? (
                    <div className="flex items-center justify-center py-8">
                      <div className="text-gray-500 text-sm">Loading available packages...</div>
                    </div>
                  ) : (() => {
                    // Group packages by name and get only the latest version of each
                    const packagesByName = new Map<string, AvailablePackage>();

                    availablePackages
                      .filter(pkg =>
                        !packages.some(releasePkg => releasePkg.name === pkg.name) &&
                        (!distribution || pkg.architecture === distribution.architecture)
                      )
                      .forEach(pkg => {
                        const existing = packagesByName.get(pkg.name);
                        if (!existing || compareVersions(pkg.version, existing.version) > 0) {
                          packagesByName.set(pkg.name, pkg);
                        }
                      });

                    const filteredPackages = Array.from(packagesByName.values())
                      .filter(pkg =>
                        packageSearchQuery === '' ||
                        pkg.name.toLowerCase().includes(packageSearchQuery.toLowerCase()) ||
                        pkg.version.toLowerCase().includes(packageSearchQuery.toLowerCase())
                      )
                      .sort((a, b) => a.name.localeCompare(b.name));

                    return filteredPackages.length === 0 ? (
                      <div className="text-center py-8 text-gray-500 text-sm">
                        {packageSearchQuery ? 'No packages match your search' : 'No available packages'}
                      </div>
                    ) : (
                      <div className="border border-gray-200 rounded-md max-h-[320px] overflow-y-auto">
                        {filteredPackages.map((pkg) => (
                          <button
                            key={pkg.id}
                            onClick={() => setSelectedAvailablePackage(pkg.id)}
                            className={`w-full text-left p-3 border-b border-gray-200 last:border-b-0 hover:bg-gray-50 transition-colors cursor-pointer ${
                              selectedAvailablePackage === pkg.id ? 'bg-blue-50 hover:bg-blue-50' : ''
                            }`}
                          >
                            <div className="flex items-start justify-between">
                              <div className="flex-1 min-w-0">
                                <div className="flex items-center space-x-2">
                                  <span className="font-medium text-gray-900">{pkg.name}</span>
                                  <span className="text-xs text-gray-500">v{pkg.version}</span>
                                  <span className={`px-2 py-0.5 text-xs font-medium rounded ${getArchColor(pkg.architecture)}`}>
                                    {pkg.architecture}
                                  </span>
                                </div>
                                <div className="text-xs text-gray-500 mt-1 truncate">
                                  {pkg.file}
                                </div>
                              </div>
                              {selectedAvailablePackage === pkg.id && (
                                <div className="flex-shrink-0 ml-2">
                                  <div className="w-5 h-5 bg-blue-600 rounded-full flex items-center justify-center">
                                    <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                                    </svg>
                                  </div>
                                </div>
                              )}
                            </div>
                          </button>
                        ))}
                      </div>
                    );
                  })()}
                </div>
              </div>

              <div className="flex justify-end space-x-3 mt-6">
                <button
                  onClick={() => setShowAddPackageModal(false)}
                  className="px-4 py-2 text-sm font-medium text-gray-600 hover:text-gray-800 hover:bg-gray-100 rounded-md transition-colors cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  onClick={handleAddPackage}
                  disabled={!selectedAvailablePackage}
                  className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                    !selectedAvailablePackage
                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                      : 'bg-blue-600 text-white hover:bg-blue-700 cursor-pointer'
                  }`}
                >
                  Add Package
                </button>
              </div>
            </div>
          </div>,
          document.body
        )}


        {/* Packages Section */}
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2">
              <Box className="w-5 h-5 text-gray-600" />
              <h2 className="text-lg font-semibold text-gray-900">Packages</h2>
              <span className="text-sm text-gray-500">({packages.length})</span>
            </div>
            <div className="flex items-center space-x-3">
              {release?.draft ? (
                <>
                  <button
                    onClick={handlePublishRelease}
                    disabled={publishing}
                    className={`flex items-center space-x-2 px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                      publishing
                        ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                        : 'bg-green-600 text-white hover:bg-green-700 cursor-pointer'
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
                    className="flex items-center space-x-2 px-4 py-2 bg-blue-600 text-white text-sm font-medium rounded-md hover:bg-blue-700 transition-colors cursor-pointer"
                  >
                    <Plus className="w-4 h-4" />
                    <span>Add Package</span>
                  </button>
                </>
              ) : !release?.yanked && (
                <button
                  onClick={() => setShowDeployModal(true)}
                  className="flex items-center space-x-2 px-4 py-2 bg-blue-600 text-white text-sm font-medium rounded-md hover:bg-blue-700 transition-colors cursor-pointer"
                >
                  <Rocket className="w-4 h-4" />
                  <span>Deploy Release</span>
                </button>
              )}
            </div>
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
                            {release?.draft && (() => {
                              const latestVersion = getLatestVersionForPackage(pkg);
                              const isUpgrading = upgradingPackages.has(pkg.id);
                              return latestVersion ? (
                                <button
                                  onClick={() => handleUpgradePackage(pkg)}
                                  disabled={isUpgrading}
                                  className={`flex items-center space-x-0.5 px-1.5 py-0.5 text-[10px] font-medium rounded-md transition-all duration-200 ${
                                    isUpgrading
                                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                                      : 'bg-green-500/20 text-green-700 border border-green-300/30 shadow-sm backdrop-blur-sm hover:bg-green-500/30 hover:border-green-400/40 hover:shadow cursor-pointer'
                                  }`}
                                  style={isUpgrading ? {} : { backdropFilter: 'blur(8px)' }}
                                  title={`Upgrade to v${latestVersion.version}`}
                                >
                                  {isUpgrading ? (
                                    <div className="w-2.5 h-2.5 border-2 border-gray-400 border-t-transparent rounded-full animate-spin"></div>
                                  ) : (
                                    <>
                                      <ArrowUp className="w-2.5 h-2.5" />
                                      <span>v{latestVersion.version}</span>
                                    </>
                                  )}
                                </button>
                              ) : null;
                            })()}
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
                            onClick={() => openReplaceModal(pkg)}
                            className="p-2 text-gray-400 hover:text-blue-600 hover:bg-blue-50 rounded-md transition-colors cursor-pointer"
                            title="Replace package version"
                          >
                            <RefreshCw className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => handleDeletePackage(pkg.id)}
                            className="p-2 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded-md transition-colors cursor-pointer"
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