'use client';

import React, { useState, useEffect, useRef } from 'react';
import { useParams, useRouter } from 'next/navigation';
import { Terminal, ArrowLeft, Pause, Play, Download, Trash2 } from 'lucide-react';
import PrivateLayout from "@/app/layouts/PrivateLayout";
import DeviceHeader from '../DeviceHeader';
import useSmithAPI from "@/app/hooks/smith-api";
import { useAuth0 } from '@auth0/auth0-react';
import { useConfig } from '@/app/hooks/config';

interface Device {
  id: number;
  serial_number: string;
  note?: string;
  last_seen: string | null;
  has_token: boolean;
  approved: boolean;
}

const LogsPage = () => {
  const params = useParams();
  const router = useRouter();
  const { callAPI } = useSmithAPI();
  const { getAccessTokenSilently } = useAuth0();
  const { config } = useConfig();
  const [device, setDevice] = useState<Device | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [service, setService] = useState<string>('');
  const [isConnecting, setIsConnecting] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const logsContainerRef = useRef<HTMLDivElement>(null);

  const serial = params.serial as string;

  useEffect(() => {
    const fetchDevice = async () => {
      try {
        const deviceData = await callAPI<Device>('GET', `/devices/${serial}`);
        if (deviceData) {
          setDevice(deviceData);
        }
      } catch (error) {
        console.error('Failed to fetch device:', error);
      }
    };
    fetchDevice();
  }, [callAPI, serial]);

  useEffect(() => {
    return () => {
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  useEffect(() => {
    if (!isPaused && logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [logs, isPaused]);

  const connectWebSocket = async () => {
    if (isConnecting || isConnected) return;

    setIsConnecting(true);

    try {
      const apiUrl = config?.API_BASE_URL || 'http://127.0.0.1:8080';
      const protocol = apiUrl.startsWith('https') ? 'wss:' : 'ws:';
      const host = apiUrl.replace(/^https?:\/\//, '');

      // Build URL with service parameter
      const serviceParam = service ? `?service=${encodeURIComponent(service)}` : '';
      const url = `${protocol}//${host}/devices/${serial}/logs${serviceParam}`;

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.onopen = () => {
        console.log('WebSocket connected');
        setIsConnected(true);
        setIsConnecting(false);
      };

      ws.onmessage = (event) => {
        if (!isPaused) {
          setLogs((prevLogs) => [...prevLogs, event.data]);
        }
      };

      ws.onerror = (error) => {
        console.error('WebSocket error:', error);
        setIsConnecting(false);
      };

      ws.onclose = () => {
        console.log('WebSocket disconnected');
        setIsConnected(false);
        setIsConnecting(false);
        wsRef.current = null;
      };
    } catch (error) {
      console.error('Failed to get access token:', error);
      setIsConnecting(false);
    }
  };

  const disconnectWebSocket = () => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    setIsConnected(false);
  };

  const togglePause = () => {
    setIsPaused(!isPaused);
  };

  const clearLogs = () => {
    setLogs([]);
  };

  const downloadLogs = () => {
    const logsText = logs.join('\n');
    const blob = new Blob([logsText], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${serial}-logs-${new Date().toISOString()}.txt`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  return (
    <PrivateLayout id="devices">
      <div className="space-y-6">
        {/* Header with Back Button */}
        <div className="flex items-center space-x-4">
          <button
            onClick={() => router.push(`/devices/${serial}`)}
            className="flex items-center space-x-2 text-gray-600 hover:text-gray-900 transition-colors"
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="text-sm font-medium">Back to Device</span>
          </button>
        </div>

        {/* Device Header */}
        {device && <DeviceHeader device={device} serial={serial} />}

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
              onClick={() => router.push(`/devices/${serial}/about`)}
              className="py-2 px-1 border-b-2 border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 font-medium text-sm transition-colors cursor-pointer"
            >
              About
            </button>
            <button
              className="py-2 px-1 border-b-2 border-blue-500 text-blue-600 font-medium text-sm"
            >
              Logs
            </button>
          </nav>
        </div>

        {/* Logs Content */}
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          {/* Controls */}
          <div className="p-4 border-b border-gray-200 bg-gray-50">
            <div className="flex items-center justify-between flex-wrap gap-4">
              <div className="flex items-center space-x-4 flex-1">
                <input
                  type="text"
                  placeholder="Service name (optional, e.g., smithd)"
                  value={service}
                  onChange={(e) => setService(e.target.value)}
                  disabled={isConnected}
                  className="px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:bg-gray-100 disabled:cursor-not-allowed flex-1 max-w-md"
                />
                {!isConnected ? (
                  <button
                    onClick={connectWebSocket}
                    disabled={isConnecting}
                    className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 disabled:bg-gray-400 disabled:cursor-not-allowed transition-colors flex items-center space-x-2"
                  >
                    <Terminal className="w-4 h-4" />
                    <span>{isConnecting ? 'Connecting...' : 'Connect'}</span>
                  </button>
                ) : (
                  <button
                    onClick={disconnectWebSocket}
                    className="px-4 py-2 bg-red-600 text-white rounded-md hover:bg-red-700 transition-colors"
                  >
                    Disconnect
                  </button>
                )}
              </div>

              <div className="flex items-center space-x-2">
                <button
                  onClick={togglePause}
                  disabled={!isConnected}
                  className="p-2 text-gray-600 hover:text-gray-900 hover:bg-gray-200 rounded disabled:text-gray-400 disabled:cursor-not-allowed transition-colors"
                  title={isPaused ? 'Resume' : 'Pause'}
                >
                  {isPaused ? <Play className="w-5 h-5" /> : <Pause className="w-5 h-5" />}
                </button>
                <button
                  onClick={downloadLogs}
                  disabled={logs.length === 0}
                  className="p-2 text-gray-600 hover:text-gray-900 hover:bg-gray-200 rounded disabled:text-gray-400 disabled:cursor-not-allowed transition-colors"
                  title="Download logs"
                >
                  <Download className="w-5 h-5" />
                </button>
                <button
                  onClick={clearLogs}
                  disabled={logs.length === 0}
                  className="p-2 text-gray-600 hover:text-gray-900 hover:bg-gray-200 rounded disabled:text-gray-400 disabled:cursor-not-allowed transition-colors"
                  title="Clear logs"
                >
                  <Trash2 className="w-5 h-5" />
                </button>
              </div>
            </div>

            {isConnected && (
              <div className="mt-2 flex items-center space-x-2">
                <div className="w-2 h-2 bg-green-500 rounded-full animate-pulse"></div>
                <span className="text-sm text-gray-600">
                  Connected {service && `to ${service}`}
                </span>
              </div>
            )}
          </div>

          {/* Logs Display */}
          <div
            ref={logsContainerRef}
            className="p-4 bg-gray-900 text-green-400 font-mono text-sm h-[600px] overflow-y-auto"
          >
            {logs.length === 0 ? (
              <div className="flex items-center justify-center h-full text-gray-500">
                <div className="text-center">
                  <Terminal className="w-12 h-12 mx-auto mb-4 opacity-50" />
                  <p>No logs yet. Click "Connect" to start streaming.</p>
                </div>
              </div>
            ) : (
              <div className="space-y-1">
                {logs.map((log, index) => (
                  <div key={index} className="whitespace-pre-wrap break-words">
                    {log}
                  </div>
                ))}
                <div ref={logsEndRef} />
              </div>
            )}
          </div>
        </div>
      </div>
    </PrivateLayout>
  );
};

export default LogsPage;
