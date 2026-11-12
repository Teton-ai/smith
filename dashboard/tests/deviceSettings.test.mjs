import assert from "node:assert/strict";
import test from "node:test";
import {
	hasUnappliedEdits,
	hasWantedSettings,
	isL4t36,
	settingSource,
	settingState,
} from "../app/utils/deviceSettings.ts";

const fixture = (over = {}) => ({
	wanted: {},
	defaults: { power_mgnt: 2 },
	resolved: {},
	unsupported: [],
	settings_version: 3,
	reported: {
		reported_at: "2026-11-12T10:00:00Z",
		applied_version: 3,
		power_mgnt: 2,
	},
	...over,
});

const reported = (over) => ({ ...fixture().reported, ...over });

for (const [name, wanted, defaults, expected] of [
	["inherits the default", {}, { power_mgnt: 2 }, "default"],
	[
		"pin differing from the default",
		{ power_mgnt: 0 },
		{ power_mgnt: 2 },
		"pinned",
	],
	[
		"pin equal to the default",
		{ power_mgnt: 2 },
		{ power_mgnt: 2 },
		"pinnedSameAsDefault",
	],
	[
		"list pin equal to the default",
		{ ntp_servers: ["a", "b"] },
		{ ntp_servers: ["a", "b"] },
		"pinnedSameAsDefault",
	],
	["no pin and no default", {}, {}, undefined],
]) {
	test(`source: ${name}`, () => {
		const field = wanted.ntp_servers ? "ntp_servers" : "power_mgnt";
		assert.equal(settingSource({ wanted, defaults }, field), expected);
	});
}

for (const [name, over, expected] of [
	[
		"unsupported wins over a read error",
		{
			unsupported: ["power_mgnt"],
			reported: reported({ read_errors: ["power_mgnt"] }),
		},
		{ state: "notUsed" },
	],
	[
		"failed carries its reason",
		{
			reported: reported({
				apply_status: [
					{
						setting: "power_mgnt",
						state: "failed",
						reason: "still 0 after 2 tries",
					},
				],
			}),
		},
		{ state: "failed", reason: "still 0 after 2 tries" },
	],
	[
		"reverted carries its reason",
		{
			reported: reported({
				apply_status: [
					{
						setting: "power_mgnt",
						state: "reverted",
						reason: "no connectivity after apply",
					},
				],
			}),
		},
		{ state: "reverted", reason: "no connectivity after apply" },
	],
	[
		"read error",
		{ reported: reported({ read_errors: ["power_mgnt"] }) },
		{ state: "unreadable" },
	],
	[
		"device behind on the version",
		{ reported: reported({ applied_version: 2, power_mgnt: 0 }) },
		{ state: "pending" },
	],
	[
		"never applied",
		{ reported: reported({ applied_version: undefined }) },
		{ state: "pending" },
	],
	[
		"current version and running differs",
		{ reported: reported({ power_mgnt: 0 }) },
		{ state: "differs" },
	],
	["current version and running matches", {}, { state: "inSync" }],
	[
		"pin wins over the default",
		{ wanted: { power_mgnt: 0 } },
		{ state: "differs" },
	],
	["not managed", { defaults: {} }, undefined],
	[
		"no report yet, even at version 0",
		{ settings_version: 0, reported: undefined },
		{ state: "pending" },
	],
	[
		"no report and not managed",
		{ defaults: {}, reported: undefined },
		undefined,
	],
]) {
	test(`state: ${name}`, () => {
		assert.deepEqual(settingState(fixture(over), "power_mgnt"), expected);
	});
}

for (const [name, version, applied, expected] of [
	["version ahead of applied", 4, 3, true],
	["never applied", 1, undefined, true],
	["versions equal", 3, 3, false],
	["fresh device at version 0", 0, undefined, false],
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
	["a pin only", { power_mgnt: 0 }, {}, true],
	["a default only", {}, { power_mgnt: 2 }, true],
	["neither", {}, {}, false],
]) {
	test(`wanted settings: ${name}`, () => {
		assert.equal(hasWantedSettings({ wanted, defaults }), expected);
	});
}
