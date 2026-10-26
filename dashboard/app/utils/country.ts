/** Whether the device runs another country than the one its access point
 *  announces. Unknown on either side is not a mismatch. */
export const countryDiffersFromAp = (
	deviceCountry: string | undefined,
	apCountry: string | undefined,
): boolean =>
	deviceCountry !== undefined &&
	apCountry !== undefined &&
	deviceCountry !== apCountry;
