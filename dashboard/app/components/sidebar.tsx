"use client";

import { Menu, X } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type React from "react";
import { useState } from "react";

export interface NavItem {
	path: string;
	label: string;
	icon: React.ComponentType<{ className?: string }>;
	external?: boolean;
}

interface SidebarProps {
	items: NavItem[];
	bottomItems?: NavItem[];
	bottomContent?: React.ReactNode;
	mobileBottomContent?: React.ReactNode;
	versionText?: string;
	children: React.ReactNode;
	className?: string;
}

function DesktopLabel({ children }: { children: React.ReactNode }) {
	return (
		<span className="text-sm font-medium whitespace-nowrap opacity-0 -translate-x-2 group-hover/sidebar:opacity-100 group-hover/sidebar:translate-x-0 transition-all duration-300 ease-out pointer-events-none group-hover/sidebar:pointer-events-auto delay-0 group-hover/sidebar:delay-500">
			{children}
		</span>
	);
}

function MobileLabel({ children }: { children: React.ReactNode }) {
	return (
		<span className="text-sm font-medium whitespace-nowrap">{children}</span>
	);
}

export default function Sidebar({
	items,
	bottomItems = [],
	bottomContent,
	mobileBottomContent,
	versionText,
	children,
	className,
}: SidebarProps) {
	const pathname = usePathname();
	const [mobileOpen, setMobileOpen] = useState(false);

	const isActive = (path: string) => pathname.startsWith(path);

	return (
		<div className={`flex min-h-screen ${className || "bg-gray-50"}`}>
			{/* Desktop sidebar */}
			<aside className="hidden md:flex flex-col fixed inset-y-0 left-0 z-40 bg-white border-r border-gray-200 w-16 hover:w-56 transition-all duration-300 ease-in-out group/sidebar delay-0 hover:delay-500">
				{/* Logo */}
				<div className="flex items-center h-16 px-4 shrink-0">
					<Link
						className="flex items-center cursor-pointer hover:opacity-80 transition-opacity duration-200"
						href="/dashboard"
					>
						<Image
							src="/logo.png"
							alt="Smith Logo"
							width={32}
							height={32}
							className="shrink-0 rounded-md shadow-sm"
							style={{
								display: "block",
								minWidth: "32px",
								minHeight: "32px",
								maxWidth: "32px",
								maxHeight: "32px",
							}}
						/>
					</Link>
					{versionText && (
						<span className="ml-3 text-[11px] text-gray-400 font-mono whitespace-nowrap opacity-0 -translate-x-2 group-hover/sidebar:opacity-100 group-hover/sidebar:translate-x-0 transition-all duration-300 ease-out delay-0 group-hover/sidebar:delay-500">
							v{versionText}
						</span>
					)}
				</div>

				{/* Nav items */}
				<nav className="flex-1 px-2 py-4 space-y-1">
					{items.map((item, index) => {
						const Icon = item.icon;
						const active = isActive(item.path);
						return (
							<Link
								key={item.path}
								href={item.path}
								className={`flex items-center h-10 rounded-md transition-all duration-200 cursor-pointer relative group/item ${
									active
										? "bg-indigo-50 text-indigo-700"
										: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
								}`}
								style={{
									transitionDelay: `${index * 20}ms`,
								}}
							>
								{/* Active accent */}
								<div
									className={`absolute left-0 top-1 bottom-1 w-[3px] rounded-r-full bg-indigo-600 transition-all duration-300 ease-out ${
										active ? "opacity-100 scale-y-100" : "opacity-0 scale-y-0"
									}`}
								/>
								<div className="flex items-center justify-center w-12 shrink-0">
									<Icon className="w-[18px] h-[18px] transition-transform duration-200 group-hover/item:scale-110" />
								</div>
								<DesktopLabel>{item.label}</DesktopLabel>
							</Link>
						);
					})}
				</nav>

				{/* Bottom section */}
				{(bottomItems.length > 0 || bottomContent) && (
					<div className="shrink-0 border-t border-gray-200 px-2 py-3 space-y-1">
						{bottomItems.map((item) => {
							const Icon = item.icon;
							return (
								<Link
									key={item.path}
									href={item.path}
									{...(item.external
										? { target: "_blank", rel: "noopener noreferrer" }
										: {})}
									className="flex items-center h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer group/item"
								>
									<div className="flex items-center justify-center w-12 shrink-0">
										<Icon className="w-[18px] h-[18px] transition-transform duration-200 group-hover/item:scale-110" />
									</div>
									<DesktopLabel>{item.label}</DesktopLabel>
								</Link>
							);
						})}
						{bottomContent}
					</div>
				)}
			</aside>

			{/* Mobile top bar */}
			<div className="md:hidden fixed top-0 left-0 right-0 z-50 bg-white border-b border-gray-200 h-14 flex items-center px-4">
				<button
					onClick={() => setMobileOpen(true)}
					className="p-2 rounded-md text-gray-600 hover:bg-gray-100 transition-colors cursor-pointer"
				>
					<Menu className="w-5 h-5" />
				</button>
				<Link href="/dashboard" className="ml-3">
					<Image
						src="/logo.png"
						alt="Smith Logo"
						width={28}
						height={28}
						className="rounded-md shadow-sm"
					/>
				</Link>
			</div>

			{/* Mobile sidebar overlay */}
			{mobileOpen && (
				<div className="md:hidden fixed inset-0 z-50 flex">
					{/* Backdrop */}
					<div
						className="fixed inset-0 bg-black/30 animate-fade-in"
						onClick={() => setMobileOpen(false)}
						onKeyDown={() => {}}
						role="presentation"
					/>
					{/* Sidebar panel */}
					<aside className="relative flex flex-col w-64 bg-white shadow-xl animate-slide-in-left">
						<button
							onClick={() => setMobileOpen(false)}
							className="absolute top-4 right-4 p-1 rounded-md text-gray-400 hover:text-gray-600 transition-colors cursor-pointer"
						>
							<X className="w-5 h-5" />
						</button>

						{/* Logo */}
						<div className="flex items-center h-16 px-4 shrink-0">
							<Link
								className="flex items-center cursor-pointer"
								href="/dashboard"
								onClick={() => setMobileOpen(false)}
							>
								<Image
									src="/logo.png"
									alt="Smith Logo"
									width={32}
									height={32}
									className="shrink-0 rounded-md shadow-sm"
								/>
							</Link>
							{versionText && (
								<span className="ml-3 text-[11px] text-gray-400 font-mono whitespace-nowrap">
									v{versionText}
								</span>
							)}
						</div>

						{/* Nav items */}
						<nav className="flex-1 px-2 py-4 space-y-1">
							{items.map((item) => {
								const Icon = item.icon;
								const active = isActive(item.path);
								return (
									<Link
										key={item.path}
										href={item.path}
										onClick={() => setMobileOpen(false)}
										className={`flex items-center h-10 rounded-md transition-colors duration-200 cursor-pointer relative ${
											active
												? "bg-indigo-50 text-indigo-700"
												: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
										}`}
									>
										{active && (
											<div className="absolute left-0 top-1 bottom-1 w-[3px] rounded-r-full bg-indigo-600" />
										)}
										<div className="flex items-center justify-center w-12 shrink-0">
											<Icon className="w-[18px] h-[18px]" />
										</div>
										<MobileLabel>{item.label}</MobileLabel>
									</Link>
								);
							})}
						</nav>

						{/* Bottom section */}
						{(bottomItems.length > 0 || mobileBottomContent) && (
							<div className="shrink-0 border-t border-gray-200 px-2 py-3 space-y-1">
								{bottomItems.map((item) => {
									const Icon = item.icon;
									return (
										<Link
											key={item.path}
											href={item.path}
											{...(item.external
												? { target: "_blank", rel: "noopener noreferrer" }
												: {})}
											className="flex items-center h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer"
										>
											<div className="flex items-center justify-center w-12 shrink-0">
												<Icon className="w-[18px] h-[18px]" />
											</div>
											<MobileLabel>{item.label}</MobileLabel>
										</Link>
									);
								})}
								{mobileBottomContent}
							</div>
						)}
					</aside>
				</div>
			)}

			{/* Main content */}
			<main className="flex-1 md:ml-16 mt-14 md:mt-0">{children}</main>
		</div>
	);
}
