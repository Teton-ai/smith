import assert from "node:assert/strict";
import test from "node:test";
import { countryDiffersFromAp } from "../app/utils/country.ts";

for (const [name, device, ap, expected] of [
	["world domain next to a Danish AP", "00", "DK", true],
	["same country", "DK", "DK", false],
	["AP announces none", "00", undefined, false],
	["device country unknown", undefined, "DK", false],
]) {
	test(name, () => {
		assert.equal(countryDiffersFromAp(device, ap), expected);
	});
}
