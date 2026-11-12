import type { BadgeVariant } from "@teton/smith-ui";
import type {
	DeviceSetting,
	DeviceSettings,
	SettingStatus,
	SettingsReport,
	SettingValues,
	WantedSettings,
} from "../api-client.ts";

/** Fields the API lets users set; the others in a report are read only. */
export type ManagedField = keyof SettingValues & DeviceSetting;

export type SettingSource = "default" | "pinned" | "pinnedSameAsDefault";

export type SettingState =
	| "notUsed"
	| "unreadable"
	| "failed"
	| "reverted"
	| "pending"
	| "differs"
	| "inSync";

export type ApplyOutcome = "applied" | "failed" | "reverted";

export type ApplySettingsResult = {
	applied_version: number;
	reverted: boolean;
	report: SettingsReport;
};

export const SETTING_LABEL: Record<ManagedField, string> = {
	wireless_mode: "Driver mode",
	power_mgnt: "Power management",
	roam_flags: "Driver roaming",
	idle_diff: "Idle RSSI diff",
	busy_diff: "Busy RSSI diff",
	scanr_exp_ms: "Scan result expiry",
	scan_int: "Scan interval",
	idle_th: "Idle RSSI threshold",
	busy_th: "Busy RSSI threshold",
	bgscan: "Background scan (bgscan)",
	nm_wifi_powersave: "Wi-Fi power save",
	nm_autoconnect_retries: "Autoconnect retries",
	nm_cloned_mac: "Connection MAC",
	nm_scan_rand_mac: "Scan MAC randomization",
	ntp_servers: "NTP servers",
	hostname: "Hostname",
};

export const SOURCE_BADGE: Record<
	SettingSource,
	{ variant: BadgeVariant; label: string }
> = {
	default: { variant: "gray", label: "Default" },
	pinned: { variant: "purple", label: "Pinned" },
	pinnedSameAsDefault: { variant: "purple", label: "Pinned (same as default)" },
};

export const STATE_BADGE: Record<
	SettingState,
	{ variant: BadgeVariant; label: string }
> = {
	notUsed: { variant: "gray", label: "Not used on this device" },
	unreadable: { variant: "yellow", label: "Couldn't read" },
	failed: { variant: "red", label: "Failed" },
	reverted: { variant: "red", label: "Reverted" },
	pending: { variant: "yellow", label: "Pending" },
	differs: { variant: "orange", label: "Not matching" },
	inSync: { variant: "green", label: "In sync" },
};

export const APPLY_OUTCOME_BADGE: Record<
	ApplyOutcome,
	{ variant: BadgeVariant; label: string }
> = {
	applied: { variant: "green", label: "Applied" },
	failed: { variant: "red", label: "Failed" },
	reverted: { variant: "red", label: "Reverted" },
};

/** The API serializes an unset `Option` as `null`, not as a missing field. */
export const isSet = <T>(value: T | null | undefined): value is T =>
	value !== undefined && value !== null;

const sameValue = (a: unknown, b: unknown) =>
	Array.isArray(a) && Array.isArray(b)
		? a.length === b.length && a.every((v, i) => v === b[i])
		: a === b;

/** The value an Apply would set: the pin, else the distribution default.
 *  `undefined` means not managed. */
export const wantedValue = <K extends ManagedField>(
	settings: Pick<DeviceSettings, "wanted" | "defaults">,
	field: K,
): NonNullable<SettingValues[K]> | undefined => {
	const value = isSet(settings.wanted[field])
		? settings.wanted[field]
		: settings.defaults[field];
	return isSet(value) ? (value as NonNullable<SettingValues[K]>) : undefined;
};

/** What the device reports it runs; `undefined` when not reported. */
export const runningValue = <K extends ManagedField>(
	settings: Pick<DeviceSettings, "reported">,
	field: K,
): NonNullable<SettingValues[K]> | undefined => {
	const value = settings.reported?.[field];
	return isSet(value) ? (value as NonNullable<SettingValues[K]>) : undefined;
};

/** Where the wanted value comes from; `undefined` when not managed. */
export const settingSource = (
	settings: Pick<DeviceSettings, "wanted" | "defaults">,
	field: ManagedField,
): SettingSource | undefined => {
	const pin = settings.wanted[field];
	if (isSet(pin)) {
		return sameValue(pin, settings.defaults[field])
			? "pinnedSameAsDefault"
			: "pinned";
	}
	return isSet(settings.defaults[field]) ? "default" : undefined;
};

/** Whether any setting has a pin or a distribution default. */
export const hasWantedSettings = (
	settings: Pick<DeviceSettings, "wanted" | "defaults">,
): boolean =>
	Object.values(settings.wanted).some(isSet) ||
	Object.values(settings.defaults).some(isSet);

/** The edits are in the DB but the device has not applied them yet. */
export const hasUnappliedEdits = (
	settings: Pick<DeviceSettings, "settings_version" | "reported">,
): boolean =>
	settings.settings_version > (settings.reported?.applied_version ?? 0);

/** Whether the device runs something other than the wanted value. */
export const runningDiffers = (
	settings: DeviceSettings,
	field: ManagedField,
): boolean => {
	const wanted = wantedValue(settings, field);
	return (
		wanted !== undefined && !sameValue(wanted, runningValue(settings, field))
	);
};

/** State of one setting, `undefined` for a setting that is not managed and
 *  has nothing to flag. Order matters: an unsupported setting is never an
 *  apply failure or a read error, an apply status wins over the version
 *  check because a revert reports the version it went back to. */
export const settingState = (
	settings: DeviceSettings,
	field: ManagedField,
): { state: SettingState; reason?: string } | undefined => {
	if (settings.unsupported.includes(field)) return { state: "notUsed" };
	const managed = wantedValue(settings, field) !== undefined;
	const reported = settings.reported;
	if (!reported) return managed ? { state: "pending" } : undefined;
	const status = reported.apply_status?.find((s) => s.setting === field);
	if (status) return { state: status.state, reason: status.reason };
	// A setting nobody manages has no state; its read error shows as the value.
	if (!managed) return undefined;
	if (reported.read_errors?.includes(field)) return { state: "unreadable" };
	// A device at version 0 is applied automatically once it has defaults
	// (FR10a), so never applied is Pending, not In sync.
	if (!isSet(reported.applied_version) || hasUnappliedEdits(settings)) {
		return { state: "pending" };
	}
	return { state: runningDiffers(settings, field) ? "differs" : "inSync" };
};

/** bgscan has no effect on L4T 36 (the driver roams); the distribution name
 *  carries the L4T release, e.g. `orin-nano-rcmptr-mini-revc-up-l4t-36.4.3`. */
export const isL4t36 = (distributionName: string | undefined): boolean =>
	/l4t-36\./.test(distributionName ?? "");

/** Groups of an `ApplySettings`, in display order. */
const WANTED_GROUPS: [keyof WantedSettings, string][] = [
	["wireless_mode", SETTING_LABEL.wireless_mode],
	["power_mgnt", SETTING_LABEL.power_mgnt],
	["roam_flags", SETTING_LABEL.roam_flags],
	["roam_tuning", "Roam tuning"],
	["nm_wifi_powersave", SETTING_LABEL.nm_wifi_powersave],
	["nm_autoconnect_retries", SETTING_LABEL.nm_autoconnect_retries],
	["mac", "MAC randomization"],
	["hostname", SETTING_LABEL.hostname],
	["ntp_servers", SETTING_LABEL.ntp_servers],
	["bgscan", SETTING_LABEL.bgscan],
];

const formatManagedValue = (value: unknown): string => {
	if (Array.isArray(value)) return value.length ? value.join(", ") : "(none)";
	if (isSet(value) && typeof value === "object") {
		return Object.entries(value as Record<string, unknown>)
			.filter(([, v]) => isSet(v))
			.map(([k, v]) => `${k} ${v}`)
			.join(", ");
	}
	return String(value);
};

/** One row per group present in an `ApplySettings`, `Set` groups with their
 *  value and `Unmanaged` ones as such; absent groups are left out. */
export const wantedRows = (
	settings: WantedSettings,
): { key: string; value: string }[] =>
	WANTED_GROUPS.flatMap(([group, label]) => {
		const managed = settings[group];
		if (!isSet(managed)) return [];
		return [
			{
				key: label,
				value:
					managed === "Unmanaged"
						? "Unmanaged"
						: formatManagedValue(managed.Set),
			},
		];
	});

/** Headline of an `ApplySettingsResult`. A refused apply is not reverted but
 *  fails every setting and carries the previous version, so failures must be
 *  checked before calling it applied. */
export const applyResultSummary = (
	result: ApplySettingsResult,
): { outcome: ApplyOutcome; text: string } => {
	if (result.reverted) {
		return {
			outcome: "reverted",
			text: `Version ${result.applied_version} reverted`,
		};
	}
	const failed = (result.report.apply_status ?? []).filter(
		(s) => s.state === "failed",
	).length;
	if (failed > 0) {
		return {
			outcome: "failed",
			text: `${failed} ${failed === 1 ? "setting" : "settings"} failed, the device runs version ${result.applied_version}`,
		};
	}
	return {
		outcome: "applied",
		text: `Version ${result.applied_version} applied`,
	};
};

/** Apply statuses as `setting: State (reason)` lines. */
export const applyStatusLines = (
	statuses: SettingStatus[] | undefined,
): string[] =>
	(statuses ?? []).map(
		(s) =>
			`${s.setting}: ${STATE_BADGE[s.state].label}${s.reason ? ` (${s.reason})` : ""}`,
	);
