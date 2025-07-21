import React, {useState} from "react";
import { Cpu, HardDrive, Home, Layers, Menu, Network, X } from "lucide-react";
import { useRouter } from "next/navigation";
import Logo from "@/app/components/logo";
import Profile from "@/app/components/profile";

export default function PrivateLayout({
                                        id,
                                     children,
                                   }: Readonly<{
  id: string
  children: React.ReactNode;
}>) {
  const router = useRouter();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const sidebarItems = [
    { id: 'dashboard', basePath: "/dashboard", label: 'Dashboard', icon: Home },
    { id: 'devices', basePath: "/devices", label: 'Devices', icon: Cpu },
    { id: 'distributions', basePath: "/dashboard", label: 'Distributions', icon: Layers },
    { id: 'modems', basePath: "/modems", label: 'Modems', icon: Network },
  ];
  const activeTab = sidebarItems.find(item => item.id === id);

  return (
    <div className="min-h-screen bg-gray-50 flex">
      {/* Sidebar */}
      <div className={`${sidebarOpen ? 'translate-x-0' : '-translate-x-full'} fixed inset-y-0 left-0 z-50 w-64 bg-white border-r border-gray-200 transform transition-transform duration-300 ease-in-out lg:translate-x-0 lg:static lg:inset-0`}>
        <div className="flex items-center justify-between h-16 px-4 border-b border-gray-200">
          <div className="flex items-center space-x-2">
            <Logo color="black" />
            <span className="text-lg font-semibold text-gray-900">Smith</span>
          </div>
          <button
            onClick={() => setSidebarOpen(false)}
            className="lg:hidden p-1 rounded-md text-gray-400 hover:text-gray-500"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <nav className="mt-5 px-2">
          <div className="space-y-1">
            {sidebarItems.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.id}
                  onClick={() => {
                    router.push(item.basePath);
                    setSidebarOpen(false);
                  }}
                  className={`${
                    id === item.id
                      ? 'bg-blue-50 border-blue-500 text-blue-700'
                      : 'border-transparent text-gray-600 hover:bg-gray-50 hover:text-gray-900'
                  } group flex items-center px-2 py-2 text-sm font-medium rounded-md border-l-4 w-full hover:cursor-pointer`}
                >
                  <Icon className="mr-3 h-5 w-5" />
                  {item.label}
                </button>
              );
            })}
          </div>
        </nav>
      </div>

      {/* Main Content */}
      <div className="flex-1 lg:pl-0">
        {/* Header */}
        <div className="bg-white border-b border-gray-200">
          <div className="px-4 sm:px-6 lg:px-8">
            <div className="flex justify-between items-center h-16">
              <div className="flex items-center">
                <button
                  onClick={() => setSidebarOpen(true)}
                  className="lg:hidden p-2 rounded-md text-gray-400 hover:text-gray-500"
                >
                  <Menu className="w-5 h-5" />
                </button>
                <h1 className="ml-2 lg:ml-0 text-2xl font-semibold text-gray-900 capitalize">
                  {activeTab?.label}
                </h1>
              </div>
              <Profile/>
            </div>
          </div>
        </div>

        {/* Page Content */}
        <main className="px-4 sm:px-6 lg:px-8 py-6">
          {children}
        </main>
      </div>

      {/* Sidebar Overlay */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 z-40 bg-gray-600 bg-opacity-75 lg:hidden"
          onClick={() => setSidebarOpen(false)}
        />
      )}
    </div>
  );
};
