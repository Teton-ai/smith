/** Whether the device runs another country than the one its access point
 *  announces. Unknown on either side (missing, or `null` from the API) is not
 *  a mismatch. */
export const countryDiffersFromAp = (
	deviceCountry: string | null | undefined,
	apCountry: string | null | undefined,
): boolean =>
	deviceCountry !== undefined &&
	deviceCountry !== null &&
	apCountry !== undefined &&
	apCountry !== null &&
	deviceCountry !== apCountry;
