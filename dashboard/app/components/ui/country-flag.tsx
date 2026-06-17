const getFlagUrl = (countryCode: string) =>
	`https://flagicons.lipis.dev/flags/4x3/${countryCode.toLowerCase()}.svg`;

/** Small country flag tile. Renders nothing if no country code is given. */
export function CountryFlag({
	countryCode,
	country,
}: {
	countryCode?: string | null;
	country?: string | null;
}) {
	if (!countryCode) return null;
	return (
		<img
			src={getFlagUrl(countryCode)}
			alt={country || "Country flag"}
			className="w-4 h-3 flex-shrink-0 rounded-sm ring-1 ring-black/5"
			onError={(e) => {
				(e.target as HTMLImageElement).style.display = "none";
			}}
		/>
	);
}
