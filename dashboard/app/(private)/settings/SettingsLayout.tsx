import { type BadgeVariant, PageContainer, SideNav } from "@teton/smith-ui";
import { Shield, Users } from "lucide-react";
import type { ReactNode } from "react";

export type SettingsTab = "users" | "roles";

// Stable color per role so a role reads the same on the users + roles pages.
export const roleVariant = (role: string): BadgeVariant => {
	if (role === "admin") return "purple";
	if (role === "default") return "gray";
	return "blue";
};

/**
 * Shared scaffold for the settings sub-pages (Users / Roles). Renders a
 * GitHub-style left sub-nav next to the active page's content.
 */
export function SettingsLayout({
	activeTab,
	children,
}: {
	activeTab: SettingsTab;
	children: ReactNode;
}) {
	return (
		<PageContainer>
			<div className="flex flex-col md:flex-row gap-6 md:gap-8">
				<aside className="md:w-56 lg:w-60 shrink-0">
					<h2 className="px-3 mb-2 text-xs font-semibold uppercase tracking-wider text-gray-400">
						Settings
					</h2>
					<SideNav
						items={[
							{
								label: "Users",
								to: "/settings/users",
								icon: Users,
								active: activeTab === "users",
							},
							{
								label: "Roles",
								to: "/settings/roles",
								icon: Shield,
								active: activeTab === "roles",
							},
						]}
					/>
				</aside>
				<div className="flex-1 min-w-0 space-y-6">{children}</div>
			</div>
		</PageContainer>
	);
}
