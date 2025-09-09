'use client';

import React, { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';
import {
  Package,
  Layers,
  Monitor,
  HardDrive,
  Cpu,
  ChevronRight,
  Users,
  CheckCircle,
  Clock,
  Computer,
  TrendingUp,
  Eye,
  EyeOff,
  Search,
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

const DistributionsPage = () => {
  const router = useRouter();
  const { callAPI, loading, error } = useSmithAPI();
  const [distributions, setDistributions] = useState<Distribution[]>([]);
  const [rollouts, setRollouts] = useState<Map<number, Rollout>>(new Map());
  const [showEmptyDistributions, setShowEmptyDistributions] = useState(false);
  const [searchTerm, setSearchTerm] = useState('');

  // Calculate overall rollout stats
  const totalDevicesAcrossAll = Array.from(rollouts.values()).reduce(
    (sum, rollout) => sum + (rollout.total_devices || 0),
    0
  );
  const updatedDevicesAcrossAll = Array.from(rollouts.values()).reduce(
    (sum, rollout) => sum + (rollout.updated_devices || 0),
    0
  );
  const pendingDevicesAcrossAll = Array.from(rollouts.values()).reduce(
    (sum, rollout) => sum + (rollout.pending_devices || 0),
    0
  );

  const overallProgress = totalDevicesAcrossAll > 0 
    ? Math.round((updatedDevicesAcrossAll / totalDevicesAcrossAll) * 100)
    : 0;

  // Filter distributions based on device count and search
  const distributionsWithDevices = distributions.filter(dist => {
    const rollout = rollouts.get(dist.id);
    return rollout && (rollout.total_devices || 0) > 0;
  });

  const distributionsWithoutDevices = distributions.filter(dist => {
    const rollout = rollouts.get(dist.id);
    return !rollout || (rollout.total_devices || 0) === 0;
  });

  // Apply search filter
  const filteredDistributions = (showEmptyDistributions ? distributions : distributionsWithDevices).filter(dist => {
    if (!searchTerm) return true;
    const searchLower = searchTerm.toLowerCase();
    return (
      dist.name.toLowerCase().includes(searchLower) ||
      dist.architecture.toLowerCase().includes(searchLower) ||
      (dist.description && dist.description.toLowerCase().includes(searchLower))
    );
  });

  const displayedDistributions = filteredDistributions;

  useEffect(() => {
    const fetchDistributions = async () => {
      const data = await callAPI<Distribution[]>('GET', '/distributions');
      if (data) {
        setDistributions(data);
        
        // Fetch rollout data for each distribution
        const rolloutPromises = data.map(async (dist) => {
          const rolloutData = await callAPI<Rollout>('GET', `/distributions/${dist.id}/rollout`);
          return { id: dist.id, rollout: rolloutData };
        });

        const rolloutResults = await Promise.all(rolloutPromises);
        const rolloutMap = new Map();
        rolloutResults.forEach(({ id, rollout }) => {
          if (rollout) {
            rolloutMap.set(id, rollout);
          }
        });
        setRollouts(rolloutMap);
      }
    };
    fetchDistributions();
  }, [callAPI]);

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

  if (loading) {
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
        {/* Search and Distribution Count */}
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center space-x-4">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
              <input
                type="text"
                placeholder="Search distributions..."
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value)}
                className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
              />
            </div>
          </div>

          <div className="mt-4 sm:mt-0 flex items-center space-x-3">
            <span className="text-sm text-gray-500">
              {loading ? 'Loading...' : `${displayedDistributions.length} distribution${displayedDistributions.length !== 1 ? 's' : ''} shown`}
            </span>
            {distributionsWithoutDevices.length > 0 && (
              <button
                onClick={() => setShowEmptyDistributions(!showEmptyDistributions)}
                className="flex items-center space-x-1 text-blue-600 hover:text-blue-800 text-sm"
              >
                {showEmptyDistributions ? <EyeOff className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
                <span>
                  {showEmptyDistributions ? 'Hide' : 'Show'} {distributionsWithoutDevices.length} empty
                </span>
              </button>
            )}
          </div>
        </div>

        {/* Distributions List */}
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          {displayedDistributions.length === 0 ? (
            <div className="p-6 text-center">
              <Layers className="w-8 h-8 text-gray-400 mx-auto mb-2" />
              <p className="text-sm text-gray-500">No distributions found</p>
            </div>
          ) : (
            <div className="divide-y divide-gray-200">
              {displayedDistributions.map((distribution) => (
                <div 
                  key={distribution.id} 
                  className="px-4 py-3 hover:bg-gray-50 cursor-pointer transition-colors"
                  onClick={() => router.push(`/distributions/${distribution.id}`)}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center space-x-3">
                      <div className={`p-1.5 rounded ${getArchColor(distribution.architecture)}`}>
                        {getArchIcon(distribution.architecture)}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center space-x-2">
                          <h4 className="text-sm font-medium text-gray-900 truncate">{distribution.name}</h4>
                          <span className={`px-1.5 py-0.5 text-xs font-medium rounded-full ${getArchColor(distribution.architecture)} flex-shrink-0`}>
                            {distribution.architecture.toUpperCase()}
                          </span>
                        </div>
                        {distribution.description && (
                          <p className="text-xs text-gray-500 truncate mt-0.5">{distribution.description}</p>
                        )}
                      </div>
                    </div>
                    <div className="flex items-center space-x-2 flex-shrink-0">
                      {rollouts.has(distribution.id) && (() => {
                        const rollout = rollouts.get(distribution.id)!;
                        if (rollout.total_devices && rollout.total_devices > 0) {
                          const progress = Math.round((rollout.updated_devices || 0) / rollout.total_devices * 100);
                          const progressColor = progress === 100 
                            ? 'bg-green-100 text-green-800' 
                            : progress >= 75 
                            ? 'bg-blue-100 text-blue-800' 
                            : progress >= 50 
                            ? 'bg-yellow-100 text-yellow-800' 
                            : 'bg-red-100 text-red-800';
                          
                          return (
                            <>
                              <div className="text-xs text-gray-700 font-medium">
                                {rollout.updated_devices || 0}/{rollout.total_devices}
                              </div>
                              <span className={`px-1.5 py-0.5 text-xs font-medium rounded ${progressColor}`}>
                                {progress}%
                              </span>
                            </>
                          );
                        }
                        return (
                          <div className="text-xs text-gray-500">0/0</div>
                        );
                      })()}
                      <ChevronRight className="w-3 h-3 text-gray-400" />
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

export default DistributionsPage;