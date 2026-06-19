import { Menu, Search, X } from "lucide-react";
import type React from "react";
import { useEffect, useState } from "react";
import { Link, useLocation } from "react-router";

export interface NavItem {
	path: string;
	label: string;
	icon: React.ComponentType<{ className?: string }>;
	external?: boolean;
	shortcut?: string;
	onClick?: () => void;
}

interface SidebarProps {
	items: NavItem[];
	bottomItems?: NavItem[];
	bottomContent?: (expanded: boolean) => React.ReactNode;
	mobileBottomContent?: React.ReactNode;
	versionText?: string;
	className?: string;
	onSearch?: () => void;
}

function useIsMac() {
	const [isMac, setIsMac] = useState(false);
	useEffect(() => {
		setIsMac(/Mac|iPhone|iPad/i.test(navigator.userAgent));
	}, []);
	return isMac;
}

function SearchTrigger({
	onClick,
	expanded,
}: {
	onClick: () => void;
	expanded: boolean;
}) {
	const isMac = useIsMac();
	return (
		<button
			onClick={onClick}
			aria-label="Open command palette"
			className="w-full flex items-center h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer group/item"
		>
			<div className="flex items-center justify-center w-12 shrink-0">
				<Search className="w-[18px] h-[18px] transition-transform duration-200 group-hover/item:scale-110" />
			</div>
			<DesktopLabel expanded={expanded}>
				<span className="flex items-center justify-between gap-2 w-full pr-2">
					Search
					<kbd className="text-[10px] border border-gray-200 rounded px-1 py-0.5 font-mono leading-none text-gray-400 bg-gray-50">
						{isMac ? "⌘K" : "Ctrl K"}
					</kbd>
				</span>
			</DesktopLabel>
		</button>
	);
}

function MobileSearchTrigger({ onClick }: { onClick: () => void }) {
	return (
		<button
			onClick={onClick}
			aria-label="Open command palette"
			className="w-full flex items-center h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer"
		>
			<div className="flex items-center justify-center w-12 shrink-0">
				<Search className="w-[18px] h-[18px]" />
			</div>
			<MobileLabel>Search</MobileLabel>
		</button>
	);
}

function DesktopLabel({
	children,
	expanded,
}: {
	children: React.ReactNode;
	expanded: boolean;
}) {
	return (
		<span
			className={`text-sm font-medium whitespace-nowrap transition-all duration-300 ease-out pointer-events-none ${
				expanded
					? "opacity-100 translate-x-0 pointer-events-auto"
					: "opacity-0 -translate-x-2"
			}`}
		>
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
	className,
	onSearch,
}: SidebarProps) {
	const { pathname } = useLocation();
	const [mobileOpen, setMobileOpen] = useState(false);
	const [expanded, setExpanded] = useState(false);

	const isActive = (path: string) => pathname.startsWith(path);

	return (
		<>
			{/* Desktop sidebar */}
			<aside
				className={`hidden md:flex flex-col shrink-0 h-screen border-r border-gray-200 transition-all duration-300 ease-in-out ${
					expanded ? "w-56" : "w-16"
				} ${className || "bg-white"}`}
			>
				{/* Logo */}
				<div className="flex items-center h-16 px-4 shrink-0">
					<button
						onClick={() => setExpanded((e) => !e)}
						className="flex items-center cursor-pointer hover:opacity-80 transition-opacity duration-200 focus:outline-none"
						aria-label={expanded ? "Collapse sidebar" : "Expand sidebar"}
					>
						<img
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
					</button>
					{versionText && (
						<span
							className={`ml-3 text-[11px] text-gray-400 font-mono whitespace-nowrap transition-all duration-300 ease-out ${
								expanded
									? "opacity-100 translate-x-0"
									: "opacity-0 -translate-x-2"
							}`}
						>
							v{versionText}
						</span>
					)}
				</div>

				{/* Nav items */}
				<nav className="flex-1 px-2 py-4 space-y-1">
					{onSearch && <SearchTrigger onClick={onSearch} expanded={expanded} />}
					{items.map((item, index) => {
						const Icon = item.icon;
						const active = item.onClick ? false : isActive(item.path);
						const itemClass = `flex items-center h-10 rounded-md transition-all duration-200 cursor-pointer relative group/item w-full text-left ${
							active
								? "bg-blue-50 text-blue-700"
								: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
						}`;
						const itemStyle = { transitionDelay: `${index * 20}ms` };
						const inner = (
							<>
								<div
									className={`absolute left-0 top-1 bottom-1 w-[3px] rounded-r-full bg-blue-600 transition-all duration-300 ease-out ${
										active ? "opacity-100 scale-y-100" : "opacity-0 scale-y-0"
									}`}
								/>
								<div className="flex items-center justify-center w-12 shrink-0">
									<Icon className="w-[18px] h-[18px] transition-transform duration-200 group-hover/item:scale-110" />
								</div>
								<DesktopLabel expanded={expanded}>
									<span className="flex items-center justify-between gap-2 w-full pr-2">
										{item.label}
										{item.shortcut && (
											<kbd className="text-[10px] border border-gray-200 rounded px-1 py-0.5 font-mono leading-none text-gray-400 bg-gray-50">
												{item.shortcut}
											</kbd>
										)}
									</span>
								</DesktopLabel>
							</>
						);
						if (item.onClick) {
							return (
								<button
									type="button"
									key={item.path}
									onClick={item.onClick}
									className={itemClass}
									style={itemStyle}
								>
									{inner}
								</button>
							);
						}
						return (
							<Link
								key={item.path}
								to={item.path}
								className={itemClass}
								style={itemStyle}
							>
								{inner}
							</Link>
						);
					})}
				</nav>

				{/* Bottom section */}
				{(bottomItems.length > 0 || bottomContent) && (
					<div className="shrink-0 border-t border-gray-200 px-2 py-3 space-y-1">
						{bottomItems.map((item) => {
							const Icon = item.icon;
							const className =
								"flex items-center h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer group/item";
							const inner = (
								<>
									<div className="flex items-center justify-center w-12 shrink-0">
										<Icon className="w-[18px] h-[18px] transition-transform duration-200 group-hover/item:scale-110" />
									</div>
									<DesktopLabel expanded={expanded}>{item.label}</DesktopLabel>
								</>
							);
							return item.external ? (
								<a
									key={item.path}
									href={item.path}
									target="_blank"
									rel="noopener noreferrer"
									className={className}
								>
									{inner}
								</a>
							) : (
								<Link key={item.path} to={item.path} className={className}>
									{inner}
								</Link>
							);
						})}
						{bottomContent?.(expanded)}
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
				<Link to="/dashboard" className="ml-3">
					<img
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
								to="/dashboard"
								onClick={() => setMobileOpen(false)}
							>
								<img
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
							{onSearch && (
								<MobileSearchTrigger
									onClick={() => {
										setMobileOpen(false);
										onSearch();
									}}
								/>
							)}
							{items.map((item) => {
								const Icon = item.icon;
								const active = item.onClick ? false : isActive(item.path);
								const itemClass = `flex items-center h-10 rounded-md transition-colors duration-200 cursor-pointer relative w-full text-left ${
									active
										? "bg-blue-50 text-blue-700"
										: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
								}`;
								const inner = (
									<>
										{active && (
											<div className="absolute left-0 top-1 bottom-1 w-[3px] rounded-r-full bg-blue-600" />
										)}
										<div className="flex items-center justify-center w-12 shrink-0">
											<Icon className="w-[18px] h-[18px]" />
										</div>
										<MobileLabel>
											<span className="flex items-center justify-between gap-2 w-full pr-2">
												{item.label}
												{item.shortcut && (
													<kbd className="text-[10px] border border-gray-200 rounded px-1 py-0.5 font-mono leading-none text-gray-400 bg-gray-50">
														{item.shortcut}
													</kbd>
												)}
											</span>
										</MobileLabel>
									</>
								);
								if (item.onClick) {
									return (
										<button
											type="button"
											key={item.path}
											onClick={() => {
												setMobileOpen(false);
												item.onClick?.();
											}}
											className={itemClass}
										>
											{inner}
										</button>
									);
								}
								return (
									<Link
										key={item.path}
										to={item.path}
										onClick={() => setMobileOpen(false)}
										className={itemClass}
									>
										{inner}
									</Link>
								);
							})}
						</nav>

						{/* Bottom section */}
						{(bottomItems.length > 0 || mobileBottomContent) && (
							<div className="shrink-0 border-t border-gray-200 px-2 py-3 space-y-1">
								{bottomItems.map((item) => {
									const Icon = item.icon;
									const className =
										"flex items-center h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer";
									const inner = (
										<>
											<div className="flex items-center justify-center w-12 shrink-0">
												<Icon className="w-[18px] h-[18px]" />
											</div>
											<MobileLabel>{item.label}</MobileLabel>
										</>
									);
									return item.external ? (
										<a
											key={item.path}
											href={item.path}
											target="_blank"
											rel="noopener noreferrer"
											className={className}
										>
											{inner}
										</a>
									) : (
										<Link key={item.path} to={item.path} className={className}>
											{inner}
										</Link>
									);
								})}
								{mobileBottomContent}
							</div>
						)}
					</aside>
				</div>
			)}
		</>
	);
}
