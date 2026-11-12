import type {
	DeviceSetting,
	DeviceSettings,
	SettingValues,
} from "../api-client.ts";

/** Fields the API lets users set; the others in a report are read only. */
export type ManagedField = keyof SettingValues;

export type SettingSource = "default" | "pinned" | "pinnedSameAsDefault";

export type SettingState =
	| "notUsed"
	| "unreadable"
	| "failed"
	| "reverted"
	| "pending"
	| "differs"
	| "inSync";

export const SOURCE_LABEL: Record<SettingSource, string> = {
	default: "Default",
	pinned: "Pinned",
	pinnedSameAsDefault: "Pinned (same as default)",
};

export const STATE_LABEL: Record<SettingState, string> = {
	notUsed: "Not used on this device",
	unreadable: "Couldn't read",
	failed: "Failed",
	reverted: "Reverted",
	pending: "Pending",
	differs: "Not matching",
	inSync: "In sync",
};

const sameValue = (a: unknown, b: unknown) =>
	Array.isArray(a) && Array.isArray(b)
		? a.length === b.length && a.every((v, i) => v === b[i])
		: a === b;

/** The value an Apply would set: the pin, else the distribution default.
 *  `undefined` means not managed. */
export const wantedValue = <K extends ManagedField>(
	settings: Pick<DeviceSettings, "wanted" | "defaults">,
	field: K,
): SettingValues[K] => settings.wanted[field] ?? settings.defaults[field];

/** Where the wanted value comes from; `undefined` when not managed. */
export const settingSource = (
	settings: Pick<DeviceSettings, "wanted" | "defaults">,
	field: ManagedField,
): SettingSource | undefined => {
	const pin = settings.wanted[field];
	if (pin !== undefined) {
		return sameValue(pin, settings.defaults[field])
			? "pinnedSameAsDefault"
			: "pinned";
	}
	return settings.defaults[field] !== undefined ? "default" : undefined;
};

/** Whether any setting has a pin or a distribution default. */
export const hasWantedSettings = (
	settings: Pick<DeviceSettings, "wanted" | "defaults">,
): boolean =>
	Object.values(settings.wanted).some((v) => v !== undefined) ||
	Object.values(settings.defaults).some((v) => v !== undefined);

/** The edits are in the DB but the device has not applied them yet. */
export const hasUnappliedEdits = (
	settings: Pick<DeviceSettings, "settings_version" | "reported">,
): boolean =>
	settings.settings_version > (settings.reported?.applied_version ?? 0);

/** Whether the reported value differs from the wanted one. */
export const runningDiffers = (
	settings: DeviceSettings,
	field: ManagedField,
): boolean => {
	const wanted = wantedValue(settings, field);
	return wanted !== undefined && !sameValue(wanted, settings.reported?.[field]);
};

/** State of one setting, `undefined` for a setting that is not managed and
 *  has nothing to flag. Order matters: an unsupported setting is never an
 *  apply failure or a read error, and a device behind on the version is
 *  Pending whatever it reads. */
export const settingState = (
	settings: DeviceSettings,
	field: ManagedField & DeviceSetting,
): { state: SettingState; reason?: string } | undefined => {
	if (settings.unsupported.includes(field)) return { state: "notUsed" };
	if (!settings.reported) {
		return wantedValue(settings, field) === undefined
			? undefined
			: { state: "pending" };
	}
	const status = settings.reported?.apply_status?.find(
		(s) => s.setting === field,
	);
	if (status) return { state: status.state, reason: status.reason };
	if (settings.reported?.read_errors?.includes(field)) {
		return { state: "unreadable" };
	}
	if (wantedValue(settings, field) === undefined) return undefined;
	if (hasUnappliedEdits(settings)) return { state: "pending" };
	return { state: runningDiffers(settings, field) ? "differs" : "inSync" };
};

/** bgscan has no effect on L4T 36 (the driver roams); the distribution name
 *  carries the L4T release, e.g. `orin-nano-rcmptr-mini-revc-up-l4t-36.4.3`. */
export const isL4t36 = (distributionName: string | undefined): boolean =>
	/l4t-36\./.test(distributionName ?? "");
