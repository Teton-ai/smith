import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
	AUTOCONNECT_RETRIES_OPTIONS,
	applyAction,
	applyErrorMessage,
	applyResultSummary,
	applyStatusLines,
	bulkEditTarget,
	defaultLabel,
	draftToChange,
	editGroups,
	groupsToReset,
	hasUnappliedEdits,
	hasWantedSettings,
	initialDraft,
	isL4t36,
	optionsWith,
	pinSeed,
	SETTING_LABEL,
	settingSource,
	settingState,
	settingsErrorMessage,
	wantedRows,
} from "../app/utils/deviceSettings.ts";

// Unset values are `null`, as the API serializes them.
const fixture = (over = {}, reportedOver = {}) => ({
	wanted: { power_mgnt: null },
	defaults: { power_mgnt: 2 },
	resolved: {},
	unsupported: [],
	settings_version: 3,
	reported: {
		reported_at: "2026-11-12T10:00:00Z",
		applied_version: 3,
		power_mgnt: 2,
		...reportedOver,
	},
	...over,
});

for (const [name, field, wanted, defaults, expected] of [
	[
		"inherits the default",
		"power_mgnt",
		{ power_mgnt: null },
		{ power_mgnt: 2 },
		"default",
	],
	[
		"pin differing from the default",
		"power_mgnt",
		{ power_mgnt: 0 },
		{ power_mgnt: 2 },
		"pinned",
	],
	[
		"pin equal to the default",
		"power_mgnt",
		{ power_mgnt: 2 },
		{ power_mgnt: 2 },
		"pinnedSameAsDefault",
	],
	[
		"list pin equal to the default",
		"ntp_servers",
		{ ntp_servers: ["a", "b"] },
		{ ntp_servers: ["a", "b"] },
		"pinnedSameAsDefault",
	],
	[
		"no pin and no default",
		"power_mgnt",
		{ power_mgnt: null },
		{ power_mgnt: null },
		undefined,
	],
]) {
	test(`source: ${name}`, () => {
		assert.equal(settingSource({ wanted, defaults }, field), expected);
	});
}

for (const [name, over, reportedOver, expected] of [
	[
		"unsupported wins over a read error",
		{ unsupported: ["power_mgnt"] },
		{ read_errors: ["power_mgnt"] },
		{ state: "notUsed" },
	],
	[
		"failed carries its reason",
		{},
		{
			apply_status: [
				{
					setting: "power_mgnt",
					state: "failed",
					reason: "still 0 after 2 tries",
				},
			],
		},
		{ state: "failed", reason: "still 0 after 2 tries" },
	],
	[
		"reverted carries its reason, though behind on the version",
		{},
		{
			applied_version: 2,
			apply_status: [
				{
					setting: "power_mgnt",
					state: "reverted",
					reason: "no connectivity after apply",
				},
			],
		},
		{ state: "reverted", reason: "no connectivity after apply" },
	],
	["read error", {}, { read_errors: ["power_mgnt"] }, { state: "unreadable" }],
	[
		"not managed, even with a read error",
		{ defaults: { power_mgnt: null } },
		{ read_errors: ["power_mgnt"] },
		undefined,
	],
	[
		"device behind on the version",
		{},
		{ applied_version: 2, power_mgnt: 0 },
		{ state: "pending" },
	],
	[
		"reported but never applied, at version 0",
		{ settings_version: 0 },
		{ applied_version: null },
		{ state: "pending" },
	],
	[
		"no report yet, even at version 0",
		{ settings_version: 0, reported: undefined },
		{},
		{ state: "pending" },
	],
	[
		"no report and not managed",
		{ defaults: { power_mgnt: null }, reported: undefined },
		{},
		undefined,
	],
	[
		"current version and running differs",
		{},
		{ power_mgnt: 0 },
		{ state: "differs" },
	],
	["current version and running matches", {}, {}, { state: "inSync" }],
	[
		"pin wins over the default",
		{ wanted: { power_mgnt: 0 } },
		{},
		{ state: "differs" },
	],
]) {
	test(`state: ${name}`, () => {
		assert.deepEqual(
			settingState(fixture(over, reportedOver), "power_mgnt"),
			expected,
		);
	});
}

for (const [name, version, applied, expected] of [
	["version ahead of applied", 4, 3, true],
	["never applied", 1, null, true],
	["versions equal", 3, 3, false],
	["fresh device at version 0", 0, null, false],
]) {
	test(`unapplied edits: ${name}`, () => {
		assert.equal(
			hasUnappliedEdits({
				settings_version: version,
				reported: { applied_version: applied },
			}),
			expected,
		);
	});
}

for (const [name, dist, expected] of [
	["L4T 36", "orin-nano-rcmptr-mini-revc-up-l4t-36.4.3", true],
	["L4T 35", "orin-nx-l4t-35.3.1", false],
	["no release", undefined, false],
]) {
	test(`l4t36: ${name}`, () => {
		assert.equal(isL4t36(dist), expected);
	});
}

for (const [name, wanted, defaults, expected] of [
	["a pin only", { power_mgnt: 0 }, { power_mgnt: null }, true],
	["a default only", { power_mgnt: null }, { power_mgnt: 2 }, true],
	["only null values", { power_mgnt: null }, { power_mgnt: null }, false],
]) {
	test(`wanted settings: ${name}`, () => {
		assert.equal(hasWantedSettings({ wanted, defaults }), expected);
	});
}

for (const [name, settings, expected] of [
	["omits null groups", { hostname: null, bgscan: null }, []],
	[
		"shows Unmanaged groups as such",
		{ hostname: "Unmanaged" },
		[{ key: "Hostname", value: "Unmanaged" }],
	],
	[
		"formats scalar, list and struct values in display order",
		{
			ntp_servers: { Set: ["a.example", "b.example"] },
			roam_tuning: { Set: { idle_diff: 15, busy_diff: null, scan_int: 5 } },
			power_mgnt: { Set: 2 },
		},
		[
			{ key: "Power management", value: "2" },
			{ key: "Roam tuning", value: "idle_diff 15, scan_int 5" },
			{ key: "NTP servers", value: "a.example, b.example" },
		],
	],
	[
		"shows an empty NTP list explicitly",
		{ ntp_servers: { Set: [] } },
		[{ key: "NTP servers", value: "(none)" }],
	],
]) {
	test(`wantedRows ${name}`, () => {
		assert.deepEqual(wantedRows(settings), expected);
	});
}

for (const [name, result, expected] of [
	[
		"applied",
		{ applied_version: 5, reverted: false, report: { apply_status: [] } },
		{ outcome: "applied", text: "Version 5 applied" },
	],
	[
		"reverted",
		{ applied_version: 5, reverted: true, report: { apply_status: [] } },
		{ outcome: "reverted", text: "Version 5 reverted" },
	],
	[
		"failed settings, as in a refused apply",
		{
			applied_version: 4,
			reverted: false,
			report: {
				apply_status: [
					{ setting: "hostname", state: "failed", reason: "no space" },
					{ setting: "bgscan", state: "failed", reason: "no space" },
				],
			},
		},
		{
			outcome: "failed",
			text: "2 settings failed, the device runs version 4",
		},
	],
]) {
	test(`applyResultSummary reports ${name}`, () => {
		assert.deepEqual(applyResultSummary(result), expected);
	});
}

for (const [name, statuses, expected] of [
	["no statuses", undefined, []],
	[
		"failed with and without a reason, and reverted",
		[
			{ setting: "hostname", state: "failed", reason: "hostnamectl exited 1" },
			{ setting: "bgscan", state: "failed", reason: "" },
			{
				setting: "power_mgnt",
				state: "reverted",
				reason: "reverted: no connectivity after apply",
			},
		],
		[
			"hostname: Failed (hostnamectl exited 1)",
			"bgscan: Failed",
			"power_mgnt: Reverted (reverted: no connectivity after apply)",
		],
	],
]) {
	test(`applyStatusLines renders ${name}`, () => {
		assert.deepEqual(applyStatusLines(statuses), expected);
	});
}

const group = (key) => editGroups({}).find((g) => g.key === key);

test("editGroups lists groups in display order, supported when resolved", () => {
	const groups = editGroups({
		power_mgnt: "Unmanaged",
		roam_flags: null,
		hostname: { Set: "h" },
	});
	assert.deepEqual(
		groups.map((g) => [g.key, g.supported]),
		[
			["wireless_mode", false],
			["power_mgnt", true],
			["roam_flags", false],
			["roam_tuning", false],
			["nm_wifi_powersave", false],
			["nm_autoconnect_retries", false],
			["mac", false],
			["hostname", true],
			["ntp_servers", false],
			["bgscan", false],
		],
	);
});

test("editGroups has no hostname group in bulk mode", () => {
	assert.equal(
		editGroups({ hostname: { Set: "h" } }, true).some(
			(g) => g.key === "hostname",
		),
		false,
	);
});

for (const [name, key, defaults, expected] of [
	["not managed", "power_mgnt", { power_mgnt: null }, "Default (not managed)"],
	["named power mgmt", "power_mgnt", { power_mgnt: 2 }, "Default (Max)"],
	[
		"named NM power save",
		"nm_wifi_powersave",
		{ nm_wifi_powersave: 2 },
		"Default (Disabled)",
	],
	[
		"autoconnect forever",
		"nm_autoconnect_retries",
		{ nm_autoconnect_retries: 0 },
		"Default (Forever)",
	],
	[
		"driver mode bits",
		"wireless_mode",
		{ wireless_mode: 1 | 4 | 32 },
		"Default (802.11b, 802.11a, AUTO)",
	],
	[
		"roam tuning with one field set",
		"roam_tuning",
		{ idle_diff: 15, busy_diff: null },
		"Default (idle_diff 15)",
	],
	[
		"mac",
		"mac",
		{ nm_cloned_mac: "preserve", nm_scan_rand_mac: false },
		"Default (preserve, scan randomization off)",
	],
	["ntp servers", "ntp_servers", { ntp_servers: ["a", "b"] }, "Default (a, b)"],
]) {
	test(`defaultLabel: ${name}`, () => {
		assert.equal(defaultLabel(group(key), defaults), expected);
	});
}

test("named options are sorted by value, negative first", () => {
	assert.deepEqual(
		AUTOCONNECT_RETRIES_OPTIONS.map((o) => [o.value, o.name]),
		[
			[-1, "NM default"],
			[0, "Forever"],
		],
	);
});

for (const [name, wanted, bulk, expected] of [
	[
		"one device resets only its pinned groups",
		{ power_mgnt: 0, bgscan: null },
		false,
		["power_mgnt"],
	],
	["bulk resets every supported group", {}, true, ["power_mgnt", "bgscan"]],
]) {
	test(`groupsToReset: ${name}`, () => {
		assert.deepEqual(
			groupsToReset(
				editGroups({ power_mgnt: "Unmanaged", bgscan: "Unmanaged" }, bulk),
				wanted,
				bulk,
			),
			expected,
		);
	});
}

test("optionsWith keeps the list when the value is listed", () => {
	assert.equal(
		optionsWith(AUTOCONNECT_RETRIES_OPTIONS, 0),
		AUTOCONNECT_RETRIES_OPTIONS,
	);
});

test("optionsWith adds a value set outside the list", () => {
	assert.deepEqual(optionsWith(AUTOCONNECT_RETRIES_OPTIONS, 4).at(-1), {
		value: 4,
		name: "4 (not a listed value)",
	});
});

for (const [name, wanted, expected] of [
	[
		"starts from the device's pin",
		{ power_mgnt: 3 },
		{ mode: "pin", value: 3 },
	],
	[
		"starts on default without a pin",
		{ power_mgnt: null },
		{ mode: "default" },
	],
]) {
	test(`initialDraft ${name}`, () => {
		assert.deepEqual(initialDraft("power_mgnt", wanted), expected);
	});
}

for (const [name, key, wanted, defaults, expected] of [
	["prefers the pin", "power_mgnt", { power_mgnt: 3 }, { power_mgnt: 1 }, 3],
	["falls back to the default", "power_mgnt", {}, { power_mgnt: 1 }, 1],
	["falls back to a blank", "wireless_mode", {}, {}, 0],
]) {
	test(`pinSeed ${name}`, () => {
		assert.equal(pinSeed(key, wanted, defaults), expected);
	});
}

for (const [name, key, draft, unsupported, expected] of [
	["default", "power_mgnt", { mode: "default" }, [], { change: "default" }],
	[
		"integer pin",
		"power_mgnt",
		{ mode: "pin", value: 2 },
		[],
		{ change: { pin: 2 } },
	],
	[
		"NTP split on commas and lines",
		"ntp_servers",
		{ mode: "pin", value: "a.org, b.org\nc.org\n\n" },
		[],
		{ change: { pin: ["a.org", "b.org", "c.org"] } },
	],
	[
		"roam tuning numbers",
		"roam_tuning",
		{
			mode: "pin",
			value: {
				idle_diff: "15",
				busy_diff: "",
				scanr_exp_ms: "1",
				scan_int: "5",
				idle_th: "30",
				busy_th: "",
			},
		},
		["busy_diff", "busy_th"],
		{
			change: {
				pin: { idle_diff: 15, scanr_exp_ms: 1, scan_int: 5, idle_th: 30 },
			},
		},
	],
	[
		"roam tuning rejects a blank supported field",
		"roam_tuning",
		{
			mode: "pin",
			value: {
				idle_diff: "15",
				busy_diff: "",
				scanr_exp_ms: "1",
				scan_int: "5",
				idle_th: "30",
				busy_th: "30",
			},
		},
		[],
		{ error: "Roam tuning: busy_diff must be a whole number" },
	],
	[
		"MAC pin",
		"mac",
		{ mode: "pin", value: { cloned_mac: " preserve ", scan_rand_mac: true } },
		[],
		{ change: { pin: { cloned_mac: "preserve", scan_rand_mac: true } } },
	],
	[
		"empty hostname",
		"hostname",
		{ mode: "pin", value: "  " },
		[],
		{ error: "Hostname is empty" },
	],
	[
		"driver mode with no standard",
		"wireless_mode",
		{ mode: "pin", value: 0 },
		[],
		{ error: "Driver mode: select at least one standard" },
	],
]) {
	test(`draftToChange: ${name}`, () => {
		assert.deepEqual(draftToChange(key, draft, unsupported), expected);
	});
}

for (const [name, err, expected] of [
	[
		"a body",
		{ response: { status: 400, data: " bad hostname " } },
		"bad hostname",
	],
	[
		"a bare 400",
		{ response: { status: 400, data: "" } },
		"The server rejected a value: check the hostname, NTP servers, MAC and bgscan.",
	],
	["no response", new Error("network"), "Failed to save the settings."],
]) {
	test(`settingsErrorMessage: ${name}`, () => {
		assert.equal(settingsErrorMessage(err), expected);
	});
}

// Real `GET /devices/{id}/settings` responses from a dev device (no
// NetworkManager, so every Wi-Fi setting is a read error).
const realResponse = (name) =>
	JSON.parse(
		readFileSync(new URL(`fixtures/${name}.json`, import.meta.url), "utf8"),
	);
const managedFields = Object.keys(SETTING_LABEL);

test("real response with nothing managed has no source or state", () => {
	const settings = realResponse("device-settings-unmanaged");
	assert.deepEqual(
		managedFields.map((f) => [
			f,
			settingSource(settings, f),
			settingState(settings, f),
		]),
		managedFields.map((f) => [f, undefined, undefined]),
	);
});

test("real response after a failed hostname pin flags only the hostname", () => {
	const settings = realResponse("device-settings-pin-failed");
	assert.deepEqual(
		managedFields
			.map((f) => [f, settingSource(settings, f), settingState(settings, f)])
			.filter(([, source, state]) => source || state),
		[
			[
				"hostname",
				"pinned",
				{ state: "failed", reason: "hostnamectl exited with exit status: 1" },
			],
		],
	);
});

for (const [name, settings, expected] of [
	[
		"offers Apply for unapplied edits",
		fixture({ settings_version: 4 }),
		{ label: "Apply", canApply: true },
	],
	[
		"offers Re-apply when the device is current",
		fixture(),
		{ label: "Re-apply", canApply: true },
	],
	[
		"refuses settings version 0",
		fixture({ settings_version: 0 }, { applied_version: 0 }),
		{ label: "Re-apply", canApply: false },
	],
]) {
	test(`applyAction ${name}`, () => {
		assert.deepEqual(applyAction(settings), expected);
	});
}

for (const [name, err, expected] of [
	[
		"nothing to apply",
		{ response: { status: 400 } },
		"Nothing to apply: no setting was ever pinned or defaulted.",
	],
	[
		"forbidden",
		{ response: { status: 403 } },
		"You don't have permission to apply settings.",
	],
	["unknown", new Error("x"), "Failed to queue the apply."],
]) {
	test(`applyErrorMessage: ${name}`, () => {
		assert.equal(applyErrorMessage(err), expected);
	});
}

test("bulkEditTarget picks the first device when all share a distribution", () => {
	assert.deepEqual(
		bulkEditTarget(
			[
				{ id: 7, release: { distribution_id: 1 } },
				{ id: 9, release: { distribution_id: 1 } },
			],
			2,
		),
		{ kind: "ready", referenceId: 7 },
	);
});

for (const [name, devices, fragment] of [
	[
		"mixed distributions",
		[
			{ id: 1, release: { distribution_id: 1 } },
			{ id: 2, release: { distribution_id: 2 } },
		],
		"more than one distribution",
	],
	[
		"a device without distribution",
		[
			{ id: 1, release: { distribution_id: 1 } },
			{ id: 2, release: null },
		],
		"has no distribution",
	],
	[
		"a selected device that is not loaded",
		[{ id: 1, release: { distribution_id: 1 } }],
		"not in the current list",
	],
]) {
	test(`bulkEditTarget blocks ${name}`, () => {
		const target = bulkEditTarget(devices, 2);
		assert.equal(target.kind, "blocked");
		assert.ok(target.message.includes(fragment));
	});
}
