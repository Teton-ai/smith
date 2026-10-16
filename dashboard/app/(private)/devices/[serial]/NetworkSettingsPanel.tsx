import { Button, InfoRow, Panel, SECTION_THEMES } from "@teton/smith-ui";
import { RefreshCw, SlidersHorizontal } from "lucide-react";
import type { ReactNode } from "react";
import {
	type Device,
	type DeviceSetting,
	type DeviceSettings,
	useGetSettingsForDevice,
	useIssueCommandsToDevices,
} from "@/app/api-client";
import { RelativeTime } from "@/app/components/RelativeTime";

/** Reported settings of one device, polled like the audit. */
export const useDeviceSettings = (serial: string) =>
	useGetSettingsForDevice(serial, { query: { refetchInterval: 30000 } });

/** Asks the device for a fresh report; the result lands a few seconds later
 *  (next poll plus the POST), so refetch shortly after. */
export const useRefreshDeviceSettings = (
	device: Device | undefined,
	refetch: () => void,
) => {
	const { mutate, isPending } = useIssueCommandsToDevices({
		mutation: {
			onSuccess: () => {
				setTimeout(refetch, 5000);
				setTimeout(refetch, 25000);
			},
			onError: (error) => {
				console.error("Failed to request a settings report:", error);
			},
		},
	});
	const refresh = () => {
		if (!device?.id) return;
		mutate({
			data: {
				devices: [device.id],
				commands: [
					{ id: -1, command: "ReportSettings", continue_on_error: false },
				],
			},
		});
	};
	return { refresh, isPending };
};

/** Renders one setting: "not used" when the distribution doesn't have it
 *  (wins over a read error, e.g. devices without the Realtek driver), "couldn't
 *  read" when the device failed to read it, otherwise the formatted value. */
export const SettingValue = ({
	settings,
	field,
	children,
}: {
	settings: DeviceSettings;
	field: DeviceSetting;
	children: ReactNode;
}) => {
	if (settings.unsupported.includes(field)) {
		return <span className="text-gray-400">Not used on this device</span>;
	}
	if (settings.reported?.read_errors?.includes(field)) {
		return <span className="text-amber-600">Couldn't read</span>;
	}
	if (
		children === undefined ||
		children === null ||
		children === false ||
		children === ""
	) {
		return <span className="text-gray-400">None</span>;
	}
	return <>{children}</>;
};

// Bits from the driver's `rtw_wireless_mode` (include/ieee80211.h).
const MODE_BITS = [
	{ bit: 1, label: "802.11b" },
	{ bit: 2, label: "802.11g" },
	{ bit: 4, label: "802.11a" },
	{ bit: 8, label: "802.11n (2.4 GHz)" },
	{ bit: 16, label: "802.11n (5 GHz)" },
	{ bit: 64, label: "802.11ac" },
];
const OTHER_MODE_BITS: Record<number, string> = { 32: "AUTO" };

const WirelessMode = ({ mode }: { mode: number }) => {
	const known = MODE_BITS.reduce((acc, m) => acc | m.bit, 0);
	const other: string[] = [];
	for (let bit = 1; bit <= mode; bit <<= 1) {
		if (mode & bit && !(known & bit)) {
			other.push(
				OTHER_MODE_BITS[bit] ? `${OTHER_MODE_BITS[bit]} (${bit})` : `${bit}`,
			);
		}
	}
	return (
		<div className="flex flex-col items-end gap-1">
			<div className="flex flex-wrap justify-end gap-x-3 gap-y-1">
				{MODE_BITS.map((m) => (
					<label
						key={m.bit}
						className="inline-flex items-center gap-1 text-xs text-gray-700"
					>
						<input
							type="checkbox"
							checked={(mode & m.bit) !== 0}
							disabled
							className="h-3.5 w-3.5"
						/>
						{m.label}
					</label>
				))}
			</div>
			{other.length > 0 && (
				<span className="text-xs text-gray-500">
					Other driver bits: {other.join(", ")}
				</span>
			)}
		</div>
	);
};

const POWER_MGNT: Record<number, string> = {
	0: "Active (no power saving)",
	1: "Min",
	2: "Max",
	3: "DTIM",
	4: "VoIP",
	5: "UAPSD WMM",
};

const NM_POWERSAVE: Record<number, string> = {
	0: "Default",
	1: "Ignore (driver decides)",
	2: "Disabled",
	3: "Enabled",
};

/** Driver signal units are dBm + 100 on both vendor driver versions. */
const threshold = (value: number) => `${value} (≈ ${value - 100} dBm)`;

const duration = (seconds: number) => {
	if (seconds >= 3600 && seconds % 3600 === 0) return `${seconds / 3600} h`;
	if (seconds >= 60 && seconds % 60 === 0) return `${seconds / 60} min`;
	return `${seconds} s`;
};

/** `simple:<short s>:<signal dBm>:<long s>` from wpa_supplicant. */
const describeBgscan = (bgscan: string) => {
	const [module, short, signal, long] = bgscan.split(":");
	if (module !== "simple" || !short || !signal || !long) return null;
	return `Scans every ${duration(Number(short))} below ${signal} dBm, every ${duration(Number(long))} above`;
};

/** Settings the device reports it runs: Realtek driver, roaming and
 *  NetworkManager defaults. Read only; editing comes later. */
const NetworkSettingsPanel = ({
	serial,
	device,
}: {
	serial: string;
	device: Device | undefined;
}) => {
	const { data: settings, isLoading, refetch } = useDeviceSettings(serial);
	const { refresh, isPending } = useRefreshDeviceSettings(device, refetch);
	const r = settings?.reported;

	return (
		<Panel
			title="Network Settings"
			icon={SlidersHorizontal}
			theme={SECTION_THEMES.purple}
			actions={
				<div className="flex items-center gap-2">
					{r && (
						<span className="text-xs text-gray-500">
							Reported <RelativeTime date={r.reported_at} />
						</span>
					)}
					<Button
						variant="soft"
						tone="gray"
						size="sm"
						loading={isPending}
						onClick={refresh}
						icon={<RefreshCw className="w-3.5 h-3.5" />}
					>
						Refresh
					</Button>
				</div>
			}
		>
			{isLoading || !settings ? (
				<div className="h-4 animate-pulse rounded bg-gray-100" />
			) : !r ? (
				<p className="text-sm text-gray-500">
					Not reported. The device needs a smithd version that reports its
					settings.
				</p>
			) : (
				<div className="space-y-5">
					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Driver
						</h5>
						<InfoRow label="Mode">
							<SettingValue settings={settings} field="wireless_mode">
								{r.wireless_mode !== undefined && (
									<WirelessMode mode={r.wireless_mode} />
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Power management">
							<SettingValue settings={settings} field="power_mgnt">
								{r.power_mgnt !== undefined &&
									(POWER_MGNT[r.power_mgnt] ?? `Unknown (${r.power_mgnt})`)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Country">
							<SettingValue settings={settings} field="country_code">
								{r.country_code === "00" ? "Unset" : r.country_code}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Roaming
						</h5>
						<InfoRow label="Active roaming">
							<SettingValue settings={settings} field="roam_flags">
								{r.roam_flags !== undefined &&
									(r.roam_flags & 0x04 ? "Enabled" : "Disable active roaming")}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Idle RSSI diff">
							<SettingValue settings={settings} field="idle_diff">
								{r.idle_diff}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Busy RSSI diff">
							<SettingValue settings={settings} field="busy_diff">
								{r.busy_diff}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Scan result expiry">
							<SettingValue settings={settings} field="scanr_exp_ms">
								{r.scanr_exp_ms !== undefined && `${r.scanr_exp_ms / 1000} s`}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Scan interval">
							<SettingValue settings={settings} field="scan_int">
								{r.scan_int !== undefined &&
									`${r.scan_int} (${duration(r.scan_int * 2)})`}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Idle RSSI threshold">
							<SettingValue settings={settings} field="idle_th">
								{r.idle_th !== undefined && threshold(r.idle_th)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Busy RSSI threshold">
							<SettingValue settings={settings} field="busy_th">
								{r.busy_th !== undefined && threshold(r.busy_th)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Background scan (bgscan)">
							<SettingValue settings={settings} field="bgscan">
								{r.bgscan && (
									<span className="flex flex-col items-end">
										<span className="font-mono text-xs">{r.bgscan}</span>
										{describeBgscan(r.bgscan) && (
											<span className="text-xs text-gray-500">
												{describeBgscan(r.bgscan)}
											</span>
										)}
									</span>
								)}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							NetworkManager defaults
						</h5>
						<InfoRow label="Wi-Fi power save">
							<SettingValue settings={settings} field="nm_wifi_powersave">
								{r.nm_wifi_powersave !== undefined &&
									(NM_POWERSAVE[r.nm_wifi_powersave] ??
										`Unknown (${r.nm_wifi_powersave})`)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Autoconnect retries">
							<SettingValue settings={settings} field="nm_autoconnect_retries">
								{r.nm_autoconnect_retries !== undefined &&
									(r.nm_autoconnect_retries === 0
										? "Forever"
										: r.nm_autoconnect_retries)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Connection MAC">
							<SettingValue settings={settings} field="nm_cloned_mac">
								{r.nm_cloned_mac && (
									<span className="font-mono text-xs">{r.nm_cloned_mac}</span>
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow label="Scan MAC randomization">
							<SettingValue settings={settings} field="nm_scan_rand_mac">
								{r.nm_scan_rand_mac !== undefined &&
									(r.nm_scan_rand_mac ? "On" : "Off")}
							</SettingValue>
						</InfoRow>
					</section>
				</div>
			)}
		</Panel>
	);
};

export default NetworkSettingsPanel;
