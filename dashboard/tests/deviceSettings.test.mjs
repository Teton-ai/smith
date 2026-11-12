import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
	applyResultSummary,
	applyStatusLines,
	hasUnappliedEdits,
	hasWantedSettings,
	isL4t36,
	SETTING_LABEL,
	settingSource,
	settingState,
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
