import assert from "node:assert/strict";
import test from "node:test";
import { versionChangeKind } from "../app/utils/versionChange.ts";

for (const [name, from, to, changed, expected] of [
	["patch upgrade", "8.0.1", "8.0.3", false, "upgrade"],
	["rollback", "8.0.3", "8.0.1", false, "rollback"],
	["numeric comparison, not lexical", "8.0.9", "8.0.10", false, "upgrade"],
	["major version rollback", "10.0.0", "9.9.9", false, "rollback"],
	["prerelease to stable", "8.0.1-rc.2", "8.0.1", false, "upgrade"],
	["stable to prerelease", "8.0.1", "8.0.1-rc.2", false, "rollback"],
	["numeric prerelease", "8.0.1-rc.9", "8.0.1-rc.10", false, "upgrade"],
	["same version", "8.0.1", "8.0.1", false, "change"],
	[
		"build metadata doesn't imply direction",
		"8.0.1+aaa",
		"8.0.1+bbb",
		false,
		"change",
	],
	["unknown naming", "nightly", "8.0.1", false, "change"],
	[
		"distribution change takes precedence",
		"8.0.1",
		"1.0.0",
		true,
		"distribution",
	],
	[
		"distribution change with same version",
		"8.0.1",
		"8.0.1",
		true,
		"distribution",
	],
]) {
	test(name, () => {
		assert.equal(versionChangeKind(from, to, changed), expected);
	});
}
