"use client";

import { useAuth0 } from "@auth0/auth0-react";
import { ChevronDown, ChevronRight, LogOut, User } from "lucide-react";
import Image from "next/image";
import { useEffect, useRef, useState } from "react";

interface ProfileProps {
	sidebar?: boolean;
	expanded?: boolean;
}

export default function Profile({ sidebar, expanded }: ProfileProps) {
	const { user, isAuthenticated, isLoading, logout } = useAuth0();
	const [isOpen, setIsOpen] = useState(false);
	const dropdownRef = useRef(null);

	// Logout if user is undefined after loading completes
	useEffect(() => {
		if (!isLoading && !user) {
			logout({ logoutParams: { returnTo: window.location.origin } });
		}
	}, [isLoading, user, logout]);

	// Close dropdown when clicking outside
	useEffect(() => {
		function handleClickOutside(event) {
			if (dropdownRef.current && !dropdownRef.current.contains(event.target)) {
				setIsOpen(false);
			}
		}

		document.addEventListener("mousedown", handleClickOutside);
		return () => {
			document.removeEventListener("mousedown", handleClickOutside);
		};
	}, []);

	if (isLoading) {
		return (
			<div
				className={
					sidebar
						? "flex items-center h-10 px-4"
						: "flex items-center space-x-2"
				}
			>
				<div className="w-8 h-8 bg-gray-200 rounded-full animate-pulse shrink-0" />
			</div>
		);
	}

	if (!isAuthenticated || !user) {
		return null;
	}

	const handleLogout = () => {
		logout({ logoutParams: { returnTo: window.location.origin } });
	};

	if (sidebar) {
		return (
			<div className="relative" ref={dropdownRef}>
				<button
					onClick={() => setIsOpen(!isOpen)}
					className="flex items-center w-full h-10 rounded-md text-gray-600 hover:bg-gray-100 hover:text-gray-900 transition-colors duration-200 cursor-pointer group/item"
				>
					<div className="flex items-center justify-center w-12 shrink-0">
						{user.picture ? (
							<Image
								src={user.picture}
								alt={user.name || "User"}
								width={28}
								height={28}
								className="rounded-full transition-transform duration-200 group-hover/item:scale-110"
							/>
						) : (
							<div className="w-7 h-7 bg-gray-300 rounded-full flex items-center justify-center transition-transform duration-200 group-hover/item:scale-110">
								<User className="w-4 h-4 text-gray-600" />
							</div>
						)}
					</div>
					{expanded ? (
						<span className="text-sm font-medium whitespace-nowrap flex items-center">
							<span className="truncate max-w-[100px]">
								{user.name || user.email}
							</span>
							<ChevronRight className="w-3.5 h-3.5 ml-1 shrink-0" />
						</span>
					) : (
						<span className="text-sm font-medium whitespace-nowrap flex items-center opacity-0 -translate-x-2 group-hover/sidebar:opacity-100 group-hover/sidebar:translate-x-0 transition-all duration-300 ease-out pointer-events-none group-hover/sidebar:pointer-events-auto">
							<span className="truncate max-w-[100px]">
								{user.name || user.email}
							</span>
							<ChevronRight className="w-3.5 h-3.5 ml-1 shrink-0" />
						</span>
					)}
				</button>

				{isOpen && (
					<div className="absolute left-full bottom-0 ml-2 w-64 bg-white rounded-md shadow-lg border border-gray-200 z-50 animate-dropdown-in">
						<div className="p-4 border-b border-gray-200">
							<div className="flex items-center space-x-3">
								{user.picture ? (
									<Image
										src={user.picture}
										alt={user.name || "User"}
										width={40}
										height={40}
										className="rounded-full"
									/>
								) : (
									<div className="w-10 h-10 bg-gray-300 rounded-full flex items-center justify-center">
										<User className="w-5 h-5 text-gray-600" />
									</div>
								)}
								<div className="flex-1 min-w-0">
									<p className="text-sm font-medium text-gray-900 truncate">
										{user.name}
									</p>
									<p className="text-sm text-gray-500 truncate">{user.email}</p>
								</div>
							</div>
						</div>

						<div className="p-1">
							<button
								onClick={handleLogout}
								className="w-full flex items-center px-3 py-2 text-sm text-gray-700 hover:bg-gray-100 rounded-md transition-colors cursor-pointer"
							>
								<LogOut className="w-4 h-4 mr-3" />
								Sign out
							</button>
						</div>
					</div>
				)}
			</div>
		);
	}

	return (
		<div className="relative" ref={dropdownRef}>
			<button
				onClick={() => setIsOpen(!isOpen)}
				className="flex items-center space-x-2 p-2 rounded-md text-gray-400 hover:text-gray-500 hover:bg-gray-50 transition-colors cursor-pointer"
			>
				{user.picture ? (
					<Image
						src={user.picture}
						alt={user.name || "User"}
						width={32}
						height={32}
						className="rounded-full"
					/>
				) : (
					<div className="w-8 h-8 bg-gray-300 rounded-full flex items-center justify-center">
						<User className="w-4 h-4 text-gray-600" />
					</div>
				)}
				<ChevronDown className="w-4 h-4" />
			</button>

			{isOpen && (
				<div className="absolute right-0 mt-3 w-64 bg-white rounded-md shadow-lg border border-gray-200 z-50 animate-dropdown-in">
					<div className="p-4 border-b border-gray-200">
						<div className="flex items-center space-x-3">
							{user.picture ? (
								<Image
									src={user.picture}
									alt={user.name || "User"}
									width={40}
									height={40}
									className="rounded-full"
								/>
							) : (
								<div className="w-10 h-10 bg-gray-300 rounded-full flex items-center justify-center">
									<User className="w-5 h-5 text-gray-600" />
								</div>
							)}
							<div className="flex-1 min-w-0">
								<p className="text-sm font-medium text-gray-900 truncate">
									{user.name}
								</p>
								<p className="text-sm text-gray-500 truncate">{user.email}</p>
							</div>
						</div>
					</div>

					<div className="p-1">
						<button
							onClick={handleLogout}
							className="w-full flex items-center px-3 py-2 text-sm text-gray-700 hover:bg-gray-100 rounded-md transition-colors cursor-pointer"
						>
							<LogOut className="w-4 h-4 mr-3" />
							Sign out
						</button>
					</div>
				</div>
			)}
		</div>
	);
}
