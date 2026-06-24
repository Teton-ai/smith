import { Shield, ShieldCheck } from "lucide-react";
import {
	Badge,
	Card,
	LabelChip,
	Panel,
	SECTION_THEMES,
} from "@/app/components/ui";
import { type RoleInfo, useGetRoles } from "../api";
import { roleVariant, SettingsLayout } from "../SettingsLayout";

// One panel per role: description, what it inherits, and the full effective
// permission set (its own permissions plus everything pulled in via inherits).
const RoleCard = ({ role }: { role: RoleInfo }) => {
	const Icon = role.name === "admin" ? ShieldCheck : Shield;
	return (
		<Panel
			icon={Icon}
			title={<span className="capitalize">{role.name}</span>}
			theme={SECTION_THEMES[role.name === "admin" ? "purple" : "gray"]}
			count={role.effective_permissions.length}
			bodyClassName="p-4 space-y-3"
		>
			<p className="text-sm text-gray-600">{role.description}</p>

			{role.inherits.length > 0 && (
				<div className="flex flex-wrap items-center gap-1.5">
					<span className="text-xs text-gray-500">Inherits</span>
					{role.inherits.map((parent) => (
						<Badge key={parent} variant={roleVariant(parent)}>
							{parent}
						</Badge>
					))}
				</div>
			)}

			<div className="space-y-1.5">
				<span className="text-xs font-medium text-gray-500">Permissions</span>
				<div className="flex flex-wrap gap-1.5">
					{role.effective_permissions.map((perm) => (
						<LabelChip
							key={`${perm.resource}:${perm.action}`}
							name={perm.resource}
							value={perm.action}
						/>
					))}
				</div>
			</div>
		</Panel>
	);
};

const SettingsRolesPage = () => {
	const { data: roles = [], isLoading, error } = useGetRoles();

	// A 403 means the signed-in user lacks the `users:read` permission (admin).
	const forbidden =
		(error as { response?: { status?: number } } | null)?.response?.status ===
		403;

	return (
		<SettingsLayout activeTab="roles">
			{forbidden ? (
				<Card className="overflow-hidden">
					<div className="p-12 text-center text-gray-500">
						<Shield className="w-12 h-12 text-gray-400 mx-auto mb-4" />
						<h3 className="text-lg font-medium text-gray-900 mb-2">
							Not authorized
						</h3>
						<p className="text-gray-500">
							You need the admin role to view roles.
						</p>
					</div>
				</Card>
			) : isLoading ? (
				<Card className="overflow-hidden">
					<div className="p-12 text-center text-gray-400">Loading...</div>
				</Card>
			) : (
				<div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
					{roles.map((role) => (
						<RoleCard key={role.name} role={role} />
					))}
				</div>
			)}
		</SettingsLayout>
	);
};

export default SettingsRolesPage;
