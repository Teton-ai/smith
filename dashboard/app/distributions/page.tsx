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
        return <Monitor className="w-3 h-3" />;
      case 'arm64':
      case 'aarch64':
        return <Cpu className="w-3 h-3" />;
      case 'armv7':
      case 'arm':
        return <HardDrive className="w-3 h-3" />;
      default:
        return <Package className="w-3 h-3" />;
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
        {/* Distributions Count */}
        <div className="flex justify-end">
          <span className="text-sm text-gray-500">
            {distributions.length} distribution{distributions.length !== 1 ? 's' : ''}
          </span>
        </div>

        {/* Distributions List */}
        <div className="bg-white rounded border border-gray-200 overflow-hidden">
          {distributions.length === 0 ? (
            <div className="p-6 text-center">
              <Layers className="w-8 h-8 text-gray-400 mx-auto mb-2" />
              <p className="text-sm text-gray-500">No distributions found</p>
            </div>
          ) : (
            <div className="divide-y divide-gray-200">
              {distributions.map((distribution) => (
                <div 
                  key={distribution.id} 
                  className="p-3 hover:bg-gray-50 cursor-pointer transition-colors"
                  onClick={() => router.push(`/distributions/${distribution.id}`)}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center space-x-3">
                      <div className={`p-1.5 rounded ${getArchColor(distribution.architecture)}`}>
                        {getArchIcon(distribution.architecture)}
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center space-x-2">
                          <h4 className="font-medium text-gray-900 truncate">{distribution.name}</h4>
                          <span className="text-xs text-gray-500">#{distribution.id}</span>
                        </div>
                        <div className="flex items-center space-x-4 mt-1 text-xs text-gray-500">
                          <span>{distribution.architecture}</span>
                          <span>{distribution.num_packages || 0} packages</span>
                          {distribution.description && (
                            <span className="truncate max-w-xs">{distribution.description}</span>
                          )}
                        </div>
                        {rollouts.has(distribution.id) && (
                          <div className="flex items-center space-x-4 mt-2">
                            {(() => {
                              const rollout = rollouts.get(distribution.id)!;
                              return (
                                <>
                                  {rollout.total_devices !== null && (
                                    <div className="flex items-center space-x-1 text-xs text-gray-600">
                                      <Computer className="w-3 h-3 text-indigo-500" />
                                      <span>{rollout.total_devices} total</span>
                                    </div>
                                  )}
                                  {rollout.updated_devices !== null && (
                                    <div className="flex items-center space-x-1 text-xs text-gray-600">
                                      <CheckCircle className="w-3 h-3 text-green-500" />
                                      <span>{rollout.updated_devices} updated</span>
                                    </div>
                                  )}
                                  {rollout.pending_devices !== null && rollout.pending_devices > 0 && (
                                    <div className="flex items-center space-x-1 text-xs text-gray-600">
                                      <Clock className="w-3 h-3 text-orange-500" />
                                      <span>{rollout.pending_devices} pending</span>
                                    </div>
                                  )}
                                </>
                              );
                            })()}
                          </div>
                        )}
                      </div>
                    </div>
                    <ChevronRight className="w-4 h-4 text-gray-400" />
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