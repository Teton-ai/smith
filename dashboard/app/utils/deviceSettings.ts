import type { BadgeVariant } from "@teton/smith-ui";
import type {
	DeviceSetting,
	DeviceSettings,
	SettingStatus,
	SettingsChanges,
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

/** What the device reports it runs; `undefined` when unreported or unset. */
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

/** Whether the device runs something other than the wanted value; false
 *  when unmanaged. */
export const runningDiffers = (
	settings: DeviceSettings,
	field: ManagedField,
): boolean => {
	const wanted = wantedValue(settings, field);
	return (
		wanted !== undefined && !sameValue(wanted, runningValue(settings, field))
	);
};

/** State of one setting, `undefined` when unmanaged with nothing to flag. Check
 *  order matters: an unsupported setting is never a failure or read error, and
 *  an apply status wins over the version check since a revert reports the
 *  version it went back to. */
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
	// A device at version 0 is applied automatically once it has defaults, so
	// never applied is Pending, not In sync.
	if (!isSet(reported.applied_version) || hasUnappliedEdits(settings)) {
		return { state: "pending" };
	}
	return { state: runningDiffers(settings, field) ? "differs" : "inSync" };
};

/** bgscan has no effect on L4T 36 (the driver roams); the distribution name
 *  carries the L4T release, e.g. `orin-nano-rcmptr-mini-revc-up-l4t-36.4.3`. */
export const isL4t36 = (distributionName: string | undefined): boolean =>
	/l4t-36\./.test(distributionName ?? "");

const GROUP_LABEL = { roam_tuning: "Roam tuning", mac: "MAC randomization" };

/** Groups of an `ApplySettings`, in display order. */
const WANTED_GROUPS: [keyof WantedSettings, string][] = [
	["wireless_mode", SETTING_LABEL.wireless_mode],
	["power_mgnt", SETTING_LABEL.power_mgnt],
	["roam_flags", SETTING_LABEL.roam_flags],
	["roam_tuning", GROUP_LABEL.roam_tuning],
	["nm_wifi_powersave", SETTING_LABEL.nm_wifi_powersave],
	["nm_autoconnect_retries", SETTING_LABEL.nm_autoconnect_retries],
	["mac", GROUP_LABEL.mac],
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

/** One row per group present in an `ApplySettings`; `Unmanaged` groups show as such. */
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

/** A refused apply is not reverted: it fails every setting and reports the
 *  previous version, so failures are checked before calling it applied. */
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

export const applyStatusLines = (
	statuses: SettingStatus[] | undefined,
): string[] =>
	(statuses ?? []).map(
		(s) =>
			`${s.setting}: ${STATE_BADGE[s.state].label}${s.reason ? ` (${s.reason})` : ""}`,
	);

export interface Flag {
	bit: number;
	label: string;
}

// Bits from the driver's `rtw_wireless_mode` (include/ieee80211.h).
export const MODE_BITS: Flag[] = [
	{ bit: 1, label: "802.11b" },
	{ bit: 2, label: "802.11g" },
	{ bit: 4, label: "802.11a" },
	{ bit: 8, label: "802.11n (2.4 GHz)" },
	{ bit: 16, label: "802.11n (5 GHz)" },
	{ bit: 64, label: "802.11ac" },
];
export const OTHER_MODE_BITS: Record<number, string> = { 32: "AUTO" };

// Bits from the driver's `roam_flags` (include/rtw_mlme.h).
export const ROAM_BITS: Flag[] = [
	{ bit: 1, label: "On link loss" },
	{ bit: 2, label: "On resume" },
	{ bit: 4, label: "Active (weak signal)" },
];

export interface NamedValue {
	name: string;
	note?: string;
}

/** `rtw_power_mgnt` values (include/rtw_pwrctrl.h). */
export const POWER_MGNT: Record<number, NamedValue> = {
	0: { name: "Active", note: "Radio never sleeps" },
	1: { name: "Min", note: "Light power saving" },
	2: { name: "Max", note: "Deepest power saving, slowest to wake" },
	3: { name: "DTIM", note: "Wakes at each DTIM beacon" },
	4: { name: "VoIP", note: "Power saving tuned for voice traffic" },
	5: { name: "UAPSD WMM", note: "Power saving driven by WMM traffic" },
};

/** NetworkManager `wifi.powersave` values. */
export const NM_POWERSAVE: Record<number, NamedValue> = {
	0: { name: "Default", note: "Follows the global default" },
	1: { name: "Ignore", note: "Left to the driver's own power saving" },
	2: { name: "Disabled" },
	3: { name: "Enabled" },
};

/** NetworkManager `autoconnect-retries`: 0 never gives up, -1 is the NM default. */
const AUTOCONNECT_RETRIES: Record<number, NamedValue> = {
	0: { name: "Forever", note: "Never stops retrying" },
	[-1]: { name: "NM default", note: "NetworkManager's own default" },
};

export type EditGroupKey = keyof SettingsChanges;

export type EditGroup = {
	key: EditGroupKey;
	label: string;
	fields: ManagedField[];
	/** The API sends `null` in `resolved` for a group the distribution cannot
	 *  manage; `Unmanaged` is still supported. */
	supported: boolean;
};

const ROAM_TUNING_FIELDS: ManagedField[] = [
	"idle_diff",
	"busy_diff",
	"scanr_exp_ms",
	"scan_int",
	"idle_th",
	"busy_th",
];

const EDIT_GROUPS: Omit<EditGroup, "supported">[] = [
	{
		key: "wireless_mode",
		label: SETTING_LABEL.wireless_mode,
		fields: ["wireless_mode"],
	},
	{
		key: "power_mgnt",
		label: SETTING_LABEL.power_mgnt,
		fields: ["power_mgnt"],
	},
	{
		key: "roam_flags",
		label: SETTING_LABEL.roam_flags,
		fields: ["roam_flags"],
	},
	{
		key: "roam_tuning",
		label: GROUP_LABEL.roam_tuning,
		fields: ROAM_TUNING_FIELDS,
	},
	{
		key: "nm_wifi_powersave",
		label: SETTING_LABEL.nm_wifi_powersave,
		fields: ["nm_wifi_powersave"],
	},
	{
		key: "nm_autoconnect_retries",
		label: SETTING_LABEL.nm_autoconnect_retries,
		fields: ["nm_autoconnect_retries"],
	},
	{
		key: "mac",
		label: GROUP_LABEL.mac,
		fields: ["nm_cloned_mac", "nm_scan_rand_mac"],
	},
	{ key: "hostname", label: SETTING_LABEL.hostname, fields: ["hostname"] },
	{
		key: "ntp_servers",
		label: SETTING_LABEL.ntp_servers,
		fields: ["ntp_servers"],
	},
	{ key: "bgscan", label: SETTING_LABEL.bgscan, fields: ["bgscan"] },
];

/** The API refuses a hostname pin on several devices, so bulk mode omits the
 *  hostname group. */
export const editGroups = (
	resolved: DeviceSettings["resolved"],
	bulk = false,
): EditGroup[] =>
	EDIT_GROUPS.filter((g) => !(bulk && g.key === "hostname")).map((g) => ({
		...g,
		supported: isSet(resolved[g.key]),
	}));

export type NamedOption = NamedValue & { value: number };

const namedOptions = (names: Record<number, NamedValue>): NamedOption[] =>
	Object.entries(names)
		.map(([value, n]) => ({ value: Number(value), ...n }))
		.sort((a, b) => a.value - b.value);

export const POWER_MGNT_OPTIONS = namedOptions(POWER_MGNT);
export const NM_POWERSAVE_OPTIONS = namedOptions(NM_POWERSAVE);
export const AUTOCONNECT_RETRIES_OPTIONS = namedOptions(AUTOCONNECT_RETRIES);

const flagNames = (
	value: number,
	flags: Flag[],
	otherNames: Record<number, string>,
) => {
	const known = new Map([
		...flags.map((f) => [f.bit, f.label] as const),
		...Object.entries(otherNames).map(([b, n]) => [Number(b), n] as const),
	]);
	const names: string[] = [];
	for (let bit = 1; bit <= value; bit <<= 1) {
		if (value & bit) names.push(known.get(bit) ?? `bit ${bit}`);
	}
	return names.length ? names.join(", ") : "none";
};

/** Only called with the group's value set (see `defaultLabel`). */
const named = (names: Record<number, NamedValue>, value: unknown) =>
	names[value as number]?.name ?? String(value);

const formatDefault = (group: EditGroup, d: SettingValues): string => {
	switch (group.key) {
		case "wireless_mode":
			return flagNames(d.wireless_mode ?? 0, MODE_BITS, OTHER_MODE_BITS);
		case "roam_flags":
			return flagNames(d.roam_flags ?? 0, ROAM_BITS, {});
		case "power_mgnt":
			return named(POWER_MGNT, d.power_mgnt);
		case "nm_wifi_powersave":
			return named(NM_POWERSAVE, d.nm_wifi_powersave);
		case "nm_autoconnect_retries":
			return named(AUTOCONNECT_RETRIES, d.nm_autoconnect_retries);
		case "ntp_servers":
			return d.ntp_servers?.length ? d.ntp_servers.join(", ") : "(none)";
		case "mac":
			return [
				d.nm_cloned_mac,
				isSet(d.nm_scan_rand_mac)
					? `scan randomization ${d.nm_scan_rand_mac ? "on" : "off"}`
					: undefined,
			]
				.filter(isSet)
				.join(", ");
		case "roam_tuning":
			return formatManagedValue(
				Object.fromEntries(
					group.fields.map((f) => [f, d[f as keyof SettingValues]]),
				),
			);
		default:
			return String(d[group.key]);
	}
};

/** What "Use distribution default" resolves to for a group. */
export const defaultLabel = (
	group: EditGroup,
	defaults: SettingValues,
): string =>
	group.fields.some((f) => isSet(defaults[f as keyof SettingValues]))
		? `Default (${formatDefault(group, defaults)})`
		: "Default (not managed)";
