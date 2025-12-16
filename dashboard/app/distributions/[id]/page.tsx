'use client';

import React, { useState } from 'react';
import { useRouter, useParams } from 'next/navigation';
import {
  Monitor,
  HardDrive,
  Cpu,
  Calendar,
  Tag,
  ArrowLeft,
  ChevronRight,
  Package,
  User,
  Plus,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import moment from 'moment';
import Link from 'next/link';
import { useCreateDistributionRelease, useGetDistributionById, useGetDistributionLatestRelease, useGetDistributionReleasePackages, useGetDistributionReleases } from '@/app/api-client';


const DistributionDetailPage = () => {
  const router = useRouter();
  const params = useParams();
  const distributionId = parseInt(params.id as string);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [selectedVersionOption, setSelectedVersionOption] = useState<string>('');
  const [isReleaseCandidate, setIsReleaseCandidate] = useState(false);
  const [creatingDraft, setCreatingDraft] = useState(false);

  const { data: distribution, isLoading: loading } = useGetDistributionById(distributionId);

  const { data: releases = [], isLoading: releasesLoading } = useGetDistributionReleases(distributionId);

  const { data: latestRelease } = useGetDistributionLatestRelease(distributionId);
  const getDistributionReleasePackages = useGetDistributionReleasePackages(latestRelease?.id as number, {query: {enabled: latestRelease?.id != null}})

  const createDistributionReleaseHook = useCreateDistributionRelease()

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

  const formatRelativeTime = (dateString: string) => {
    return moment(dateString).fromNow();
  };

  // Get the latest non-yanked release to use as base (includes drafts)
  const getLatestRelease = () => {
    return releases.find(release => !release.yanked) || releases[0];
  };

  // Parse version and generate options
  const getVersionOptions = () => {
    const baseRelease = getLatestRelease();
    if (!baseRelease) return [];

    const version = baseRelease.version;
    // Try to parse semantic version (e.g., "1.2.3")
    const match = version.match(/^(\d+)\.(\d+)\.(\d+)/);
    
    if (match) {
      const [, major, minor, patch] = match;
      return [
        {
          type: 'PATCH',
          version: `${major}.${minor}.${parseInt(patch) + 1}`,
          description: 'Bug fixes and small changes'
        },
        {
          type: 'MINOR', 
          version: `${major}.${parseInt(minor) + 1}.0`,
          description: 'New features, backwards compatible'
        },
        {
          type: 'MAJOR',
          version: `${parseInt(major) + 1}.0.0`,
          description: 'Significant new features, may include breaking changes'
        }
      ];
    }
    
    // Fallback for non-semantic versions
    return [
      {
        type: 'NEW',
        version: `${version}.1`,
        description: 'New version'
      }
    ];
  };

  const handleCreateDraft = async () => {
    if (creatingDraft || getDistributionReleasePackages.data == null || !selectedVersionOption) return;

    setCreatingDraft(true);
    try {
      const finalVersion = isReleaseCandidate ? `${selectedVersionOption}-rc` : selectedVersionOption;
      
      const newRelease = await createDistributionReleaseHook.mutateAsync({
        distributionId,
        data: {
          packages: getDistributionReleasePackages.data.map((p) => p.id),
          version: finalVersion
        }
      });

      if (newRelease) {
        router.push(`/releases/${newRelease}`);
      }
    } catch (error: any) {
      console.error('Failed to create draft release:', error);
      alert(`Failed to create draft release: ${error?.message || 'Unknown error'}`);
    } finally {
      setCreatingDraft(false);
      setShowCreateModal(false);
      setSelectedVersionOption('');
      setIsReleaseCandidate(false);
    }
  };

  const openCreateModal = () => {
    const latestRelease = getLatestRelease();
    if (latestRelease) {
      const options = getVersionOptions();
      if (options.length > 0) {
        setSelectedVersionOption(options[0].version); // Default to first option (PATCH)
      }
      setIsReleaseCandidate(false);
      setShowCreateModal(true);
    }
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

  return (
    <PrivateLayout id="distributions">
      <div className="space-y-6">
        {/* Header with Back Button */}
        <div className="flex items-center space-x-4">
          <Link
            href='/distributions'
            className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors cursor-pointer"
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="text-sm font-medium">Back to Distributions</span>
          </Link>
        </div>

        {/* Distribution Header */}
        <div className="bg-white rounded-lg border border-gray-200 p-4">
          <div className="flex items-center space-x-3">
            <div className={`p-2 rounded ${getArchColor(distribution.architecture)}`}>
              {getArchIcon(distribution.architecture)}
            </div>
            <div className="flex-1">
              <div className="flex items-center space-x-3">
                <h1 className="text-xl font-bold text-gray-900">{distribution.name}</h1>
                <span className={`px-2 py-1 text-xs font-medium rounded-full ${getArchColor(distribution.architecture)}`}>
                  {distribution.architecture.toUpperCase()}
                </span>
              </div>
              {distribution.description && (
                <p className="text-sm text-gray-600">{distribution.description}</p>
              )}
            </div>
          </div>
        </div>

        {/* Create Release Modal */}
        {showCreateModal && (
          <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
            <div className="bg-white rounded-lg shadow-xl p-6 w-[480px]">
              <div className="mb-6">
                <h3 className="text-lg font-semibold text-gray-900 mb-2">Draft New Release</h3>
                <p className="text-sm text-gray-600">
                  Based on <span className="font-medium">{getLatestRelease()?.version}</span> by{' '}
                  {getLatestRelease()?.user_email || (getLatestRelease()?.user_id ? `User #${getLatestRelease()?.user_id}` : 'Unknown')}
                  {getLatestRelease()?.draft && ' (draft)'}
                </p>
              </div>

              <div className="space-y-3 mb-6">
                {getVersionOptions().map((option) => (
                  <label
                    key={option.version}
                    className={`flex items-center p-3 border rounded-lg cursor-pointer transition-all duration-200 ${
                      selectedVersionOption === option.version
                        ? 'border-green-500 bg-green-50 shadow-sm'
                        : 'border-gray-200 hover:border-gray-300 hover:bg-gray-50'
                    }`}
                  >
                    <input
                      type="radio"
                      name="version"
                      value={option.version}
                      checked={selectedVersionOption === option.version}
                      onChange={(e) => setSelectedVersionOption(e.target.value)}
                      className="w-4 h-4 text-green-600 border-gray-300 focus:ring-green-500"
                    />
                    <div className="ml-3 flex-1">
                      <div className="flex items-center space-x-3">
                        <span className="font-mono text-sm font-medium text-gray-900">
                          {option.version}
                        </span>
                        <span className="px-2 py-1 text-xs font-medium bg-gray-100 text-gray-700 rounded">
                          {option.type}
                        </span>
                      </div>
                      <p className="text-xs text-gray-600 mt-1">{option.description}</p>
                    </div>
                  </label>
                ))}
              </div>

              <div className="mb-6">
                <label className="flex items-center space-x-3 p-3 border border-gray-200 rounded-lg hover:bg-gray-50 cursor-pointer transition-colors">
                  <input
                    type="checkbox"
                    checked={isReleaseCandidate}
                    onChange={(e) => setIsReleaseCandidate(e.target.checked)}
                    className="w-4 h-4 text-orange-600 border-gray-300 rounded focus:ring-orange-500"
                  />
                  <div className="flex-1">
                    <div className="flex items-center space-x-2">
                      <span className="text-sm font-medium text-gray-900">Release Candidate</span>
                      <span className="px-2 py-1 text-xs font-medium bg-orange-100 text-orange-700 rounded">
                        RC
                      </span>
                    </div>
                    <p className="text-xs text-gray-600 mt-1">
                      Adds "-rc" suffix: {selectedVersionOption ? `${selectedVersionOption}${isReleaseCandidate ? '-rc' : ''}` : 'x.x.x'}
                    </p>
                  </div>
                </label>
              </div>

              <div className="flex justify-end space-x-3">
                <button
                  onClick={() => {
                    setShowCreateModal(false);
                    setSelectedVersionOption('');
                    setIsReleaseCandidate(false);
                  }}
                  className="px-4 py-2 text-sm font-medium text-gray-600 hover:text-gray-800 hover:bg-gray-100 rounded-md transition-all duration-200"
                  disabled={creatingDraft}
                >
                  Cancel
                </button>
                <button
                  onClick={handleCreateDraft}
                  disabled={creatingDraft || !selectedVersionOption}
                  className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                    creatingDraft || !selectedVersionOption
                      ? 'bg-gray-200 text-gray-400 cursor-not-allowed'
                      : 'bg-green-600 text-white hover:bg-green-700'
                  }`}
                >
                  {creatingDraft ? (
                    <div className="flex items-center space-x-2">
                      <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
                      <span>Creating...</span>
                    </div>
                  ) : (
                    'Create Draft'
                  )}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Releases Section */}
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2">
              <Tag className="w-5 h-5 text-gray-600" />
              <h2 className="text-lg font-semibold text-gray-900">Releases</h2>
              <span className="text-sm text-gray-500">({releases.length})</span>
            </div>
            {releases.length > 0 && (
              <button
                onClick={openCreateModal}
                className="flex items-center space-x-2 px-4 py-2 bg-green-600 text-white text-sm font-medium rounded-md hover:bg-green-700 transition-colors cursor-pointer"
              >
                <Plus className="w-4 h-4" />
                <span>Draft New Release</span>
              </button>
            )}
          </div>
          
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
                  <Link
                    key={release.id} 
                    className="block p-4 hover:bg-gray-50 cursor-pointer transition-colors"
                    href={`/releases/${release.id}`}
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center space-x-3">
                        <div className="p-2 bg-gray-100 text-gray-600 rounded">
                          <Tag className="w-4 h-4" />
                        </div>
                        <div>
                          <div className="flex items-center space-x-2">
                            <h4 className="font-medium text-gray-900">{release.version}</h4>
                            {latestRelease && latestRelease.id === release.id && (
                              <span className="px-2 py-1 text-xs font-medium bg-green-100 text-green-800 rounded-full">
                                Latest
                              </span>
                            )}
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
                          <div className="flex items-center space-x-3 mt-1 text-xs text-gray-500">
                            <div className="flex items-center space-x-1">
                              <Calendar className="w-3 h-3" />
                              <span>{formatRelativeTime(release.created_at)}</span>
                            </div>
                            <div className="flex items-center space-x-1">
                              <User className="w-3 h-3" />
                              <span>{release.user_email || (release.user_id ? `User #${release.user_id}` : 'Unknown')}</span>
                            </div>
                          </div>
                        </div>
                      </div>
                      <div className="flex items-center space-x-4">
                        <ChevronRight className="w-4 h-4 text-gray-400" />
                      </div>
                    </div>
                  </Link>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default DistributionDetailPage;
