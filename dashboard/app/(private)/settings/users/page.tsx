import { Badge, Button, type ButtonTone, Card } from "@teton/smith-ui";
import { Search, Shield, User } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { useGetUsers } from "../api";
import { roleVariant, SettingsLayout } from "../SettingsLayout";

// Role → button tone, for the filter toggles (matches the badge colors).
const roleTone = (role: string): ButtonTone => {
	if (role === "admin") return "purple";
	if (role === "default") return "gray";
	return "blue";
};

// Column template shared by the table header and every row so they stay aligned.
// All tracks are proportional (fr) — not `auto` — so the header grid and each
// row grid (which are separate grids) compute identical column widths.
const COLS = "grid grid-cols-[2fr_2fr_1fr] gap-4 items-center";

const UserSkeleton = () => (
	<div className={`${COLS} px-4 py-3 animate-pulse`}>
		<div className="flex items-center space-x-3">
			<div className="w-8 h-8 bg-gray-200 rounded-full flex-shrink-0" />
			<div className="h-4 bg-gray-300 rounded w-48" />
		</div>
		<div className="flex gap-1">
			<div className="h-5 bg-gray-200 rounded w-16" />
		</div>
		<div className="h-3 bg-gray-200 rounded w-16 justify-self-end" />
	</div>
);

const formatDate = (date: string) =>
	new Date(date).toLocaleDateString(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
	});

const SettingsUsersPage = () => {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const [searchTerm, setSearchTerm] = useState("");
	const searchInputRef = useRef<HTMLInputElement>(null);

	const { data: users = [], isLoading: usersLoading, error } = useGetUsers();

	useEffect(() => {
		setSearchTerm(searchParams.get("search") || "");
	}, [searchParams]);

	// Focus search on "/" keypress, like the devices page.
	useEffect(() => {
		function handleKeyDown(e: KeyboardEvent) {
			if (e.key !== "/") return;
			if (e.ctrlKey || e.metaKey || e.altKey) return;
			const target = e.target as HTMLElement;
			if (
				target.tagName === "INPUT" ||
				target.tagName === "TEXTAREA" ||
				target.tagName === "SELECT" ||
				target.isContentEditable
			)
				return;
			e.preventDefault();
			searchInputRef.current?.focus();
		}
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, []);

	// Filter state lives in the URL so a view is shareable.
	const roleFilter = searchParams.get("role") ?? "";

	const updateParams = (mutate: (params: URLSearchParams) => void) => {
		const params = new URLSearchParams(searchParams);
		mutate(params);
		const qs = params.toString();
		navigate(qs ? `/settings/users?${qs}` : "/settings/users", {
			preventScrollReset: true,
		});
	};

	const handleSearchChange = (value: string) => {
		setSearchTerm(value);
		updateParams((params) => {
			if (value) params.set("search", value);
			else params.delete("search");
		});
	};

	const toggleRole = (name: string) =>
		updateParams((params) => {
			if (!name || roleFilter === name) params.delete("role");
			else params.set("role", name);
		});

	// A 403 means the signed-in user lacks the `users:read` permission (admin).
	const forbidden =
		(error as { response?: { status?: number } } | null)?.response?.status ===
		403;

	// Roles to offer as filters: those actually assigned to a user.
	const roleNames = Array.from(new Set(users.flatMap((u) => u.roles))).sort();

	const term = searchTerm.toLowerCase();
	const visibleUsers = users
		.filter((user) => {
			const matchesSearch =
				term === "" ||
				(user.email ?? "").toLowerCase().includes(term) ||
				user.roles.some((role) => role.toLowerCase().includes(term));
			const matchesRole = roleFilter === "" || user.roles.includes(roleFilter);
			return matchesSearch && matchesRole;
		})
		// Always sorted by role: the user's first (alphabetically-first) role,
		// with role-less users last, then by email within a role.
		.sort((a, b) => {
			const ar = a.roles[0] ?? "~";
			const br = b.roles[0] ?? "~";
			return (
				ar.localeCompare(br) || (a.email ?? "").localeCompare(b.email ?? "")
			);
		});

	return (
		<SettingsLayout activeTab="users">
			{/* Search + filters in one row, like the devices page */}
			<div className="flex flex-wrap items-center gap-3">
				<div className="relative">
					<Search className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 w-4 h-4 pointer-events-none" />
					<input
						ref={searchInputRef}
						type="text"
						placeholder="Search users..."
						value={searchTerm}
						onChange={(e) => handleSearchChange(e.target.value)}
						className="pl-10 pr-10 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-sm text-gray-900 placeholder-gray-400"
					/>
					{!searchTerm && (
						<kbd className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 text-xs border border-gray-300 rounded px-1 py-0.5 font-mono leading-none pointer-events-none">
							/
						</kbd>
					)}
				</div>

				{/* Role filter toggles */}
				{!usersLoading && !forbidden && roleNames.length > 0 && (
					<div className="flex flex-wrap items-center gap-1.5">
						<Button
							variant={roleFilter === "" ? "solid" : "soft"}
							tone={roleFilter === "" ? "blue" : "gray"}
							onClick={() => toggleRole("")}
						>
							All
						</Button>
						{roleNames.map((name) => (
							<Button
								key={name}
								className="capitalize"
								variant={roleFilter === name ? "solid" : "soft"}
								tone={roleFilter === name ? roleTone(name) : "gray"}
								onClick={() => toggleRole(name)}
							>
								{name}
							</Button>
						))}
					</div>
				)}

				{!usersLoading && !forbidden && (
					<span className="ml-auto text-sm text-gray-500">
						{visibleUsers.length} user{visibleUsers.length !== 1 ? "s" : ""}
					</span>
				)}
			</div>

			{/* Users table */}
			<Card className="overflow-hidden">
				{forbidden ? (
					<div className="p-12 text-center text-gray-500">
						<Shield className="w-12 h-12 text-gray-400 mx-auto mb-4" />
						<h3 className="text-lg font-medium text-gray-900 mb-2">
							Not authorized
						</h3>
						<p className="text-gray-500">
							You need the admin role to view users.
						</p>
					</div>
				) : (
					<>
						{/* Header row */}
						<div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
							<div
								className={`${COLS} text-xs font-medium text-gray-500 uppercase tracking-wide`}
							>
								<div>User</div>
								<div>Roles</div>
								<div
									className="justify-self-end"
									title="When the user first signed in"
								>
									Joined
								</div>
							</div>
						</div>

						{usersLoading ? (
							<div className="divide-y divide-gray-100">
								{Array.from({ length: 6 }, (_, i) => (
									<UserSkeleton key={i} />
								))}
							</div>
						) : visibleUsers.length === 0 ? (
							<div className="p-12 text-center text-gray-500">
								<User className="w-12 h-12 text-gray-400 mx-auto mb-4" />
								<h3 className="text-lg font-medium text-gray-900 mb-2">
									{searchTerm || roleFilter
										? "No matching users found"
										: "No users found"}
								</h3>
								<p className="text-gray-500">
									{searchTerm || roleFilter
										? "Try adjusting your search or filters."
										: "No users have signed in yet."}
								</p>
							</div>
						) : (
							<div className="divide-y divide-gray-100">
								{visibleUsers.map((user) => (
									<div
										key={user.id}
										className={`${COLS} px-4 py-3 hover:bg-gray-50 transition-colors`}
									>
										<div className="flex items-center space-x-3 min-w-0">
											<div className="w-8 h-8 rounded-full bg-gray-100 flex items-center justify-center flex-shrink-0">
												<User className="w-4 h-4 text-gray-500" />
											</div>
											<div className="flex items-baseline gap-2 min-w-0">
												<span className="text-sm text-gray-900 truncate">
													{user.email ?? (
														<span className="text-gray-400 italic">
															no email
														</span>
													)}
												</span>
												<span className="text-xs text-gray-400 flex-shrink-0">
													#{user.id}
												</span>
											</div>
										</div>
										<div className="flex flex-wrap items-center gap-1">
											{user.roles.length === 0 ? (
												<span className="text-xs text-gray-400 italic">
													no roles
												</span>
											) : (
												user.roles.map((role) => (
													<Badge key={role} variant={roleVariant(role)}>
														{role}
													</Badge>
												))
											)}
										</div>
										<span
											className="text-xs text-gray-500 tabular-nums justify-self-end"
											title="When the user first signed in"
										>
											{formatDate(user.created_at)}
										</span>
									</div>
								))}
							</div>
						)}
					</>
				)}
			</Card>
		</SettingsLayout>
	);
};

export default SettingsUsersPage;
