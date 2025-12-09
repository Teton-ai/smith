import React from "react";
import { Cpu, Home, Layers, FileText, Globe, Smartphone } from "lucide-react";
import { useRouter } from "next/navigation";
import Image from "next/image";
import Profile from "@/app/components/profile";
import Link from "next/link";

export default function PrivateLayout({
  id,
  children,
}: Readonly<{
  id: string
  children: React.ReactNode;
}>) {
  const router = useRouter();
  const navigationItems = [
    { id: 'dashboard', basePath: "/dashboard", label: 'Dashboard', icon: Home },
    { id: 'devices', basePath: "/devices", label: 'Devices', icon: Cpu },
    { id: 'distributions', basePath: "/distributions", label: 'Distributions', icon: Layers },
    { id: 'ip-addresses', basePath: "/ip-addresses", label: 'IP Addresses', icon: Globe },
    { id: 'modems', basePath: "/modems", label: 'Modems', icon: Smartphone },
  ];

  return (
    <div className="min-h-screen bg-gray-50">
      {/* Top Navigation Bar */}
      <header className="bg-white border-b border-gray-200 sticky top-0 z-50">
        <div className="mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex items-center justify-between h-16">
            {/* Left side - Logo and Navigation */}
            <div className="flex items-center space-x-8">
              {/* Logo */}
              <div
                className="cursor-pointer hover:opacity-80 transition-opacity duration-200"
                onClick={() => router.push('/dashboard')}
              >
                <Image
                  src="/logo.png"
                  alt="Smith Logo"
                  width={32}
                  height={32}
                  className="shrink-0 rounded-md shadow-sm"
                />
              </div>
              
              {/* Navigation Items */}
              <nav className="hidden md:flex space-x-1">
                {navigationItems.map((item) => {
                  const Icon = item.icon;
                  const isActive = id === item.id;
                  return (
                    <Link
                      key={item.id}
                      href={item.basePath}
                      className={`${
                        isActive
                          ? 'text-gray-900 bg-gray-100'
                          : 'text-gray-600 hover:text-gray-900 hover:bg-gray-50'
                      } px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200 flex items-center space-x-2 cursor-pointer`}
                    >
                      <Icon className="w-4 h-4" />
                      <span>{item.label}</span>
                    </Link>
                  );
                })}
              </nav>
            </div>

            {/* Right side - Docs and Profile */}
            <div className="flex items-center space-x-3">
              {/* Docs Button */}
              <button
                onClick={() => window.open('https://docs.smith.teton.ai', '_blank')}
                className="text-gray-600 hover:text-gray-900 hover:bg-gray-50 px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200 flex items-center space-x-2 cursor-pointer"
              >
                <FileText className="w-4 h-4" />
                <span className="hidden sm:inline">Docs</span>
              </button>
              <Profile />
            </div>
          </div>
        </div>

        {/* Mobile Navigation */}
        <div className="md:hidden border-t border-gray-200 bg-white">
          <nav className="px-4 py-2 space-y-1">
            {navigationItems.map((item) => {
              const Icon = item.icon;
              const isActive = id === item.id;
              return (
                <Link
                  key={item.id}
                  href={item.basePath}
                  className={`${
                    isActive
                      ? 'text-gray-900 bg-gray-100'
                      : 'text-gray-600 hover:text-gray-900 hover:bg-gray-50'
                  } group flex items-center px-2 py-2 text-base font-medium rounded-md w-full transition-colors duration-200 cursor-pointer`}
                >
                  <Icon className="mr-3 h-5 w-5" />
                  {item.label}
                </Link>
              );
            })}
          </nav>
        </div>
      </header>

      {/* Main Content */}
      <main className="mx-auto px-4 sm:px-6 lg:px-8 py-6">
        {children}
      </main>
    </div>
  );
};
