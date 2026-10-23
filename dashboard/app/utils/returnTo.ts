export function getReturnToPath(
	requested: string | null,
	origin: string,
): string {
	if (!requested) return "/dashboard";
	try {
		const url = new URL(requested, origin);
		// A leading // would become a network URL again when passed to navigate.
		if (url.origin !== origin || url.pathname.startsWith("//")) {
			return "/dashboard";
		}
		return `${url.pathname}${url.search}${url.hash}`;
	} catch {
		return "/dashboard";
	}
}
