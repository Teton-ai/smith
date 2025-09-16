'use client';

import React, { useState, useEffect } from 'react';
import { useParams, useRouter } from 'next/navigation';
import {
  Cpu,
  ChevronRight,
  CheckCircle,
  XCircle,
  AlertTriangle,
  RefreshCw,
} from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import useSmithAPI from "@/app/hooks/smith-api";
import DeviceHeader from '../DeviceHeader';

interface Command {
  device: number;
  serial_number: string;
  cmd_id: number;
  issued_at: string;
  cmd_data: {
    FreeForm?: {
      cmd: string;
    };
    UpdateVariables?: {
      variables: Record<string, string>;
    };
  };
  cancelled: boolean;
  fetched: boolean;
  fetched_at: string | null;
  response_id: number | null;
  response_at: string | null;
  response: {
    FreeForm?: {
      stderr: string;
      stdout: string;
    };
  } | string | null;
  status: number;
}

interface CommandsResponse {
  commands: Command[];
  next: string | null;
  previous: string | null;
}

const CommandsPage = () => {
  const params = useParams();
  const router = useRouter();
  const { callAPI } = useSmithAPI();
  const [commands, setCommands] = useState<Command[]>([]);
  const [device, setDevice] = useState<any>(null);
  const [loading, setLoading] = useState(true);

  const serial = params.serial as string;

  useEffect(() => {
    const fetchData = async () => {
      setLoading(true);
      try {
        const [commandsResponse, deviceData] = await Promise.all([
          callAPI<CommandsResponse>('GET', `/devices/${serial}/commands`),
          callAPI('GET', `/devices/${serial}`)
        ]);
        if (commandsResponse) {
          setCommands(commandsResponse.commands);
        }
        if (deviceData) {
          setDevice(deviceData);
        }
      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, [callAPI, serial]);

  const formatTimeAgo = (date: string) => {
    const now = new Date();
    const past = new Date(date);
    const diff = now.getTime() - past.getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d ago`;
    if (hours > 0) return `${hours}h ago`;
    return `${minutes}m ago`;
  };

  const getCommandStatus = (cmd: Command) => {
    if (cmd.cancelled) return 'cancelled';
    if (!cmd.fetched) return 'pending';
    if (!cmd.response_at) return 'running';
    return cmd.status === 0 ? 'completed' : 'failed';
  };

  const getCommandStatusIcon = (status: string) => {
    switch (status) {
      case 'completed':
        return <CheckCircle className="w-4 h-4 text-green-500" />;
      case 'running':
        return <RefreshCw className="w-4 h-4 text-blue-500 animate-spin" />;
      case 'failed':
        return <XCircle className="w-4 h-4 text-red-500" />;
      case 'cancelled':
        return <XCircle className="w-4 h-4 text-gray-500" />;
      case 'pending':
        return <RefreshCw className="w-4 h-4 text-yellow-500" />;
      default:
        return <AlertTriangle className="w-4 h-4 text-yellow-500" />;
    }
  };

  const getCommandStatusColor = (status: string) => {
    switch (status) {
      case 'completed':
        return 'text-green-600 bg-green-50 border-green-200';
      case 'running':
        return 'text-blue-600 bg-blue-50 border-blue-200';
      case 'failed':
        return 'text-red-600 bg-red-50 border-red-200';
      case 'cancelled':
        return 'text-gray-600 bg-gray-50 border-gray-200';
      case 'pending':
        return 'text-yellow-600 bg-yellow-50 border-yellow-200';
      default:
        return 'text-yellow-600 bg-yellow-50 border-yellow-200';
    }
  };

  const getCommandText = (cmd: Command) => {
    if (cmd.cmd_data.FreeForm) {
      return cmd.cmd_data.FreeForm.cmd;
    }
    if (cmd.cmd_data.UpdateVariables) {
      return 'Update Environment Variables';
    }
    return 'Unknown Command';
  };

  const getDuration = (cmd: Command) => {
    if (!cmd.fetched_at || !cmd.response_at) return null;
    const start = new Date(cmd.fetched_at);
    const end = new Date(cmd.response_at);
    return ((end.getTime() - start.getTime()) / 1000).toFixed(1);
  };

  const getOutput = (cmd: Command) => {
    if (typeof cmd.response === 'string') return cmd.response;
    if (cmd.response?.FreeForm) {
      const { stdout, stderr } = cmd.response.FreeForm;
      if (stderr && stdout) return `STDOUT:\n${stdout}\n\nSTDERR:\n${stderr}`;
      if (stderr) return stderr;
      if (stdout) return stdout;
      return 'No output';
    }
    return null;
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
          <span className="text-gray-900 font-medium">Commands</span>
        </div>

        {/* Device Header */}
        <DeviceHeader device={device} serial={serial} />

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
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
            >
              Commands
            </button>
            <button
              onClick={() => router.push(`/devices/${serial}/about`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              About
            </button>
          </nav>
        </div>

        {/* Commands Content */}
        <div className="space-y-6">
          <div className="bg-white rounded-lg border border-gray-200">
            <div className="p-6 border-b border-gray-200">
              <h3 className="text-lg font-semibold text-gray-900">Command History</h3>
              <p className="text-sm text-gray-600 mt-1">Latest executed commands on this device</p>
            </div>
            {loading ? (
              <div className="divide-y divide-gray-200">
                {[...Array(5)].map((_, index) => (
                  <div key={index} className="p-6 animate-pulse">
                    <div className="flex items-start space-x-3">
                      <div className="w-4 h-4 bg-gray-200 rounded-full" />
                      <div className="flex-1 space-y-2">
                        <div className="h-4 bg-gray-200 rounded w-3/4" />
                        <div className="h-3 bg-gray-200 rounded w-1/2" />
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            ) : commands.length === 0 ? (
              <div className="p-12 text-center text-gray-500">
                <AlertTriangle className="w-12 h-12 text-gray-400 mx-auto mb-4" />
                <h3 className="text-lg font-medium text-gray-900 mb-2">No commands found</h3>
                <p className="text-gray-500">No commands have been executed on this device yet.</p>
              </div>
            ) : (
              <div className="divide-y divide-gray-200">
                {commands.map((cmd) => {
                  const status = getCommandStatus(cmd);
                  const duration = getDuration(cmd);
                  const output = getOutput(cmd);
                  
                  return (
                    <div key={cmd.cmd_id} className="p-6 hover:bg-gray-50">
                      <div className="flex items-start justify-between">
                        <div className="flex items-start space-x-3 flex-1">
                          {getCommandStatusIcon(status)}
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center space-x-3 mb-2">
                              <code className="text-sm font-mono bg-gray-100 px-2 py-1 rounded break-all text-gray-900">
                                {getCommandText(cmd)}
                              </code>
                              <span className={`inline-flex px-2 py-1 text-xs font-medium rounded-full border ${getCommandStatusColor(status)}`}>
                                {status}
                              </span>
                            </div>
                            <div className="flex items-center space-x-4 mt-2 text-sm text-gray-700">
                              <span>Issued {formatTimeAgo(cmd.issued_at)}</span>
                              {duration && (
                                <span>Duration: {duration}s</span>
                              )}
                              <span>Exit code: {cmd.status}</span>
                              <span className="font-mono text-xs">#{cmd.cmd_id}</span>
                            </div>
                            {cmd.cmd_data.UpdateVariables && (
                              <details className="mt-3">
                                <summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800">
                                  Show variables ({Object.keys(cmd.cmd_data.UpdateVariables.variables).length})
                                </summary>
                                <div className="mt-2 text-xs bg-gray-50 p-3 rounded overflow-x-auto">
                                  {Object.entries(cmd.cmd_data.UpdateVariables.variables).map(([key, value]) => (
                                    <div key={key} className="flex items-start space-x-2 py-1">
                                      <span className="font-mono font-semibold text-gray-900 min-w-0 flex-shrink-0">{key}:</span>
                                      <span className="font-mono text-gray-800 break-all">{value}</span>
                                    </div>
                                  ))}
                                </div>
                              </details>
                            )}
                            {output && (
                              <details className="mt-3">
                                <summary className="text-sm text-blue-600 cursor-pointer hover:text-blue-800">
                                  Show output
                                </summary>
                                <pre className="mt-2 text-xs bg-gray-900 text-gray-100 p-3 rounded overflow-x-auto whitespace-pre-wrap">
                                  {output}
                                </pre>
                              </details>
                            )}
                          </div>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default CommandsPage;