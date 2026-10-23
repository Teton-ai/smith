import assert from "node:assert/strict";
import test from "node:test";
import { getReturnToPath } from "../app/utils/returnTo.ts";

const origin = "https://smith.teton.ai";

for (const [name, requested, expected] of [
	["missing destination", null, "/dashboard"],
	["empty destination", "", "/dashboard"],
	[
		"command URL with query and hash",
		"/devices/GM18Z49A/commands?id=7#output",
		"/devices/GM18Z49A/commands?id=7#output",
	],
	[
		"same-origin absolute URL",
		`${origin}/commands?id=123#output`,
		"/commands?id=123#output",
	],
	["normalized internal path", "/devices/../commands", "/commands"],
	["external URL", "https://evil.com/commands", "/dashboard"],
	["external network URL", "//evil.com/commands", "/dashboard"],
	["different scheme", "http://smith.teton.ai/commands", "/dashboard"],
	["different port", `${origin}:8443/commands`, "/dashboard"],
	["tab between slashes", "/\t/evil.com", "/dashboard"],
	["newline between slashes", "/\n/evil.com", "/dashboard"],
	["carriage return between slashes", "/\r/evil.com", "/dashboard"],
	["backslash network URL", "/\\evil.com", "/dashboard"],
	["same-origin double-slash path", `${origin}//evil.com`, "/dashboard"],
	["invalid URL", "https://[invalid", "/dashboard"],
	["non-HTTP scheme", "javascript:alert(1)", "/dashboard"],
]) {
	test(name, () => {
		assert.equal(getReturnToPath(requested, origin), expected);
	});
}

test("encoded controls are validated after query-string decoding", () => {
	for (const control of ["%09", "%0A", "%0D"]) {
		const params = new URLSearchParams(`returnTo=/${control}/evil.com`);
		assert.equal(getReturnToPath(params.get("returnTo"), origin), "/dashboard");
	}
});
