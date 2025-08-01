'use client';

import React, { useState, useEffect } from 'react';
import {
  Package,
  Layers,
  Monitor,
  Search,
  HardDrive,
  Cpu,
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

const DistributionsPage = () => {
  const { callAPI, loading, error } = useSmithAPI();
  const [distributions, setDistributions] = useState<Distribution[]>([]);
  const [filteredDistributions, setFilteredDistributions] = useState<Distribution[]>([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [filterArch, setFilterArch] = useState('all');

  useEffect(() => {
    const fetchDistributions = async () => {
      const data = await callAPI<Distribution[]>('GET', '/distributions');
      if (data) {
        setDistributions(data);
      }
    };
    fetchDistributions();
  }, [callAPI]);

  useEffect(() => {
    let filtered = distributions;

    if (searchTerm) {
      filtered = filtered.filter(dist =>
        dist.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
        (dist.description && dist.description.toLowerCase().includes(searchTerm.toLowerCase())) ||
        dist.architecture.toLowerCase().includes(searchTerm.toLowerCase())
      );
    }

    if (filterArch !== 'all') {
      filtered = filtered.filter(dist => dist.architecture === filterArch);
    }

    setFilteredDistributions(filtered);
  }, [distributions, searchTerm, filterArch]);

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

  const uniqueArchitectures = [...new Set(distributions.map(dist => dist.architecture))];

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
        {/* Filters */}
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center space-x-4">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
              <input
                type="text"
                placeholder="Search distributions..."
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value)}
                className="pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm"
              />
            </div>

            <div className="relative">
              <select
                value={filterArch}
                onChange={(e) => setFilterArch(e.target.value)}
                className="appearance-none pl-3 pr-8 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm bg-white"
              >
                <option value="all">All architectures</option>
                {uniqueArchitectures.map(arch => (
                  <option key={arch} value={arch}>{arch}</option>
                ))}
              </select>
            </div>
          </div>

          <div className="mt-4 sm:mt-0">
            <span className="text-sm text-gray-500">
              {filteredDistributions.length} distribution{filteredDistributions.length !== 1 ? 's' : ''}
            </span>
          </div>
        </div>

        {/* Distributions List */}
        <div className="bg-white rounded border border-gray-200 overflow-hidden">
          {filteredDistributions.length === 0 ? (
            <div className="p-6 text-center">
              <Layers className="w-8 h-8 text-gray-400 mx-auto mb-2" />
              <p className="text-sm text-gray-500">No distributions found</p>
            </div>
          ) : (
            <div className="divide-y divide-gray-200">
              {filteredDistributions.map((distribution) => (
                <div key={distribution.id} className="p-3">
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