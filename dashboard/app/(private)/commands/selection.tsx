import { isAxiosError } from "axios";
import { type RefObject, useEffect, useRef } from "react";

/** Change selection without losing filters or other page parameters. */
export function selectionSearch(
	current: URLSearchParams,
	values: Record<string, string | null>,
) {
	const next = new URLSearchParams(current);
	for (const [key, value] of Object.entries(values)) {
		if (value == null) next.delete(key);
		else next.set(key, value);
	}
	return `?${next.toString()}`;
}

export function validCommandId(value: string | null): value is string {
	return (
		value != null && /^[1-9]\d*$/.test(value) && Number(value) <= 2147483647
	);
}

export function validBundleId(value: string | null): value is string {
	return (
		value != null &&
		/^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}$/i.test(value)
	);
}

/** Reveal a linked selection in its normal history position, once per navigation.
 * Polling and manual scrolling must not repeatedly move the reader's viewport. */
export function useRevealSelection({
	selectionKey,
	containerRef,
	enabled,
	hasNextPage,
	isFetching,
	isFetchNextPageError,
	fetchNextPage,
}: {
	selectionKey: string | null;
	containerRef: RefObject<HTMLDivElement | null>;
	enabled: boolean;
	hasNextPage: boolean;
	isFetching: boolean;
	isFetchNextPageError: boolean;
	fetchNextPage: () => Promise<unknown>;
}) {
	const progress = useRef({ key: selectionKey, revealed: false });
	useEffect(() => {
		if (progress.current.key !== selectionKey) {
			progress.current = { key: selectionKey, revealed: false };
		}
		if (selectionKey === null || progress.current.revealed) return;
		// CommandListRow uses this border to mark the selected history row.
		const row =
			containerRef.current?.querySelector<HTMLElement>(".border-l-blue-500");
		if (row) {
			row.scrollIntoView({ block: "nearest" });
			progress.current.revealed = true;
		} else if (enabled && hasNextPage && !isFetching && !isFetchNextPageError) {
			void fetchNextPage();
		}
	}, [
		selectionKey,
		containerRef,
		enabled,
		hasNextPage,
		isFetching,
		isFetchNextPageError,
		fetchNextPage,
	]);
}

export function SelectionMessage({
	invalid,
	loading,
	error,
	kind,
}: {
	invalid: boolean;
	loading: boolean;
	error: unknown;
	kind: "command" | "bundle";
}) {
	const status = isAxiosError(error) ? error.response?.status : undefined;
	const message = invalid
		? `Invalid ${kind} ID`
		: loading
			? `Loading ${kind}…`
			: status === 404
				? `${kind === "command" ? "Command" : "Bundle"} not found`
				: status === 401 || status === 403
					? `You do not have access to this ${kind}`
					: error
						? `Failed to load ${kind}. Retrying automatically…`
						: `Select a ${kind} to see its details`;
	return (
		<output className="flex items-center justify-center h-full p-5 text-gray-500 text-sm">
			{message}
		</output>
	);
}
