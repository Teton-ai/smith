"use client";

import { useAuth0 } from "@auth0/auth0-react";
import { ChevronDown, LogOut, User } from "lucide-react";
import Image from "next/image";
import { useEffect, useRef, useState } from "react";

export default function Profile() {
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
			<div className="flex items-center space-x-2">
				<div className="w-8 h-8 bg-gray-200 rounded-full animate-pulse"></div>
				<div className="w-4 h-4 bg-gray-200 rounded animate-pulse"></div>
			</div>
		);
	}

	if (!isAuthenticated || !user) {
		return null;
	}

	const handleLogout = () => {
		logout({ logoutParams: { returnTo: window.location.origin } });
	};

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
				<div className="absolute right-0 mt-3 w-64 bg-white rounded-md shadow-lg border border-gray-200 z-50">
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
