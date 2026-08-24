/** Minimal shape `groupScanResults` needs from a scanned access point. Both the
 * persisted `WifiScanResult` (device Network page) and the raw `WifiScan`
 * command response (Commands tab) satisfy this without remapping, aside from
 * `band`, which the command response doesn't carry and callers must derive
 * (see `deriveBand`). */
export interface ScanApLike {
	bssid: string;
	ssid?: string | null;
	security?: string | null;
	band?: string | null;
	channel?: number | null;
	signal?: number | null;
	rate?: number | null;
}

/** One row per (SSID, security) pair the scan saw: APs sharing an SSID but
 *  differing in security are different networks, so they get separate rows.
 *  Hidden APs (no SSID) all collapse into a single group regardless of
 *  security, since there is nothing more specific to group them by. */
export interface ScanGroup<T extends ScanApLike = ScanApLike> {
	key: string;
	ssid: string | null;
	/** Raw scan security string (e.g. "WPA1 WPA2"); null = open. Meaningless
	 *  for the hidden group, which mixes securities. */
	security: string | null;
	bestSignal: number | null;
	bands: string[];
	aps: T[];
}

export const HIDDEN_GROUP_KEY = "\0hidden";

export function groupScanResults<T extends ScanApLike>(
	results: T[],
): ScanGroup<T>[] {
	const groups = new Map<string, ScanGroup<T>>();
	for (const r of results) {
		const key =
			r.ssid == null ? HIDDEN_GROUP_KEY : `${r.ssid}\0${r.security ?? ""}`;
		let group = groups.get(key);
		if (!group) {
			group = {
				key,
				ssid: r.ssid ?? null,
				security: r.ssid == null ? null : (r.security ?? null),
				bestSignal: null,
				bands: [],
				aps: [],
			};
			groups.set(key, group);
		}
		group.aps.push(r);
	}

	for (const group of groups.values()) {
		group.aps.sort((a, b) => (b.signal ?? -1) - (a.signal ?? -1));
		group.bestSignal = group.aps[0]?.signal ?? null;
		group.bands = [
			...new Set(
				group.aps.map((ap) => ap.band).filter((b): b is string => !!b),
			),
		];
	}

	const visible = [...groups.values()]
		.filter((g) => g.ssid != null)
		.sort((a, b) => (b.bestSignal ?? -1) - (a.bestSignal ?? -1));
	const hidden = groups.get(HIDDEN_GROUP_KEY);
	return hidden ? [...visible, hidden] : visible;
}

/** Mirrors the api's persisted-scan band derivation (`api/src/device/route.rs`,
 * `GET /devices/{serial}/wifi-scan`), for callers working from the raw
 * `WifiScan` command response, which only carries `channel`. */
export function deriveBand(channel: number | null): string | null {
	if (channel == null) return null;
	return channel <= 14 ? "2.4 GHz" : "5 GHz";
}

export function SignalBar({ value }: { value: number | null }) {
	const pct = value ?? 0;
	const color =
		value == null
			? "bg-gray-200"
			: pct >= 60
				? "bg-green-500"
				: pct >= 35
					? "bg-yellow-500"
					: "bg-orange-500";
	return (
		<span className="inline-flex items-center gap-2 min-w-[110px]">
			<span className="flex-1 h-1.5 min-w-[56px] rounded-full bg-gray-100 overflow-hidden">
				<span
					className={`block h-full rounded-full ${color}`}
					style={{ width: `${Math.max(pct, value == null ? 0 : 2)}%` }}
				/>
			</span>
			<span className="text-xs text-gray-500 tabular-nums w-9 text-right">
				{value != null ? `${value}%` : "—"}
			</span>
		</span>
	);
}
