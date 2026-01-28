"use client";

import { Bell, Cpu, FileText, Globe, Home, Layers, Smartphone } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type React from "react";
import Profile from "@/app/components/profile";
import { useGetDevices } from "../api-client";

export default function PrivateLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	const pathname = usePathname();
	const { data: unapprovedDevices } = useGetDevices(
		{ approved: false },
		{ query: { refetchInterval: 5000 } },
	);
	const pendingCount = unapprovedDevices?.length || 0;

	const navigationItems = [
		{ basePath: "/dashboard", label: "Dashboard", icon: Home },
		{ basePath: "/devices", label: "Devices", icon: Cpu },
		{
			basePath: "/distributions",
			label: "Distributions",
			icon: Layers,
		},
		{
			basePath: "/ip-addresses",
			label: "IP Addresses",
			icon: Globe,
		},
		{ basePath: "/modems", label: "Modems", icon: Smartphone },
	];
	const isActive = (path: string) => {
		return pathname.startsWith(path);
	};

	return (
		<div className="min-h-screen bg-gray-50">
			{/* Top Navigation Bar */}
			<header className="bg-white border-b border-gray-200 sticky top-0 z-50">
				<div className="mx-auto px-4 sm:px-6 lg:px-8">
					<div className="flex items-center justify-between h-16">
						{/* Left side - Logo and Navigation */}
						<div className="flex items-center space-x-8">
							{/* Logo */}
							<Link
								className="block hover:opacity-80 transition-opacity duration-200"
								href="/dashboard"
							>
								<Image
									src="/logo.png"
									alt="Smith Logo"
									width={32}
									height={32}
									className="shrink-0 rounded-md shadow-sm"
								/>
							</Link>

							{/* Navigation Items */}
							<nav className="hidden md:flex space-x-1">
								{navigationItems.map((item) => {
									const Icon = item.icon;
									const active = isActive(item.basePath);
									return (
										<Link
											key={item.basePath}
											href={item.basePath}
											className={`${
												active
													? "text-gray-900 bg-gray-100"
													: "text-gray-600 hover:text-gray-900 hover:bg-gray-50"
											} block px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200 flex items-center space-x-2`}
										>
											<Icon className="w-4 h-4" />
											<span>{item.label}</span>
										</Link>
									);
								})}
							</nav>
						</div>

						{/* Right side - Notifications, Docs and Profile */}
						<div className="flex items-center space-x-3">
							{/* Approvals Notification */}
							<Link
								href="/approvals"
								className={`relative block p-2 rounded-md transition-colors duration-200 ${
									isActive("/approvals")
										? "text-gray-900 bg-gray-100"
										: "text-gray-600 hover:text-gray-900 hover:bg-gray-50"
								}`}
							>
								<Bell className="w-5 h-5" />
								{pendingCount > 0 && (
									<span className="absolute -top-1 -right-1 flex items-center justify-center min-w-[18px] h-[18px] px-1 text-xs font-bold text-white bg-orange-500 rounded-full">
										{pendingCount > 99 ? "99+" : pendingCount}
									</span>
								)}
							</Link>
							{/* Docs Link */}
							<Link
								href="https://docs.smith.teton.ai"
								rel="noopener noreferrer"
								className="block text-gray-600 hover:text-gray-900 hover:bg-gray-50 px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200 flex items-center space-x-2"
							>
								<FileText className="w-4 h-4" />
								<span className="hidden sm:inline">Docs</span>
							</Link>
							<Profile />
						</div>
					</div>
				</div>

				{/* Mobile Navigation */}
				<div className="md:hidden border-t border-gray-200 bg-white">
					<nav className="px-4 py-2 space-y-1">
						{navigationItems.map((item) => {
							const Icon = item.icon;
							const active = isActive(item.basePath);
							return (
								<Link
									key={item.basePath}
									href={item.basePath}
									className={`${
										active
											? "text-gray-900 bg-gray-100"
											: "text-gray-600 hover:text-gray-900 hover:bg-gray-50"
									} group flex items-center px-2 py-2 text-base font-medium rounded-md w-full transition-colors duration-200`}
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
			<main className="mx-auto px-4 sm:px-6 lg:px-8 py-6">{children}</main>
		</div>
	);
}
