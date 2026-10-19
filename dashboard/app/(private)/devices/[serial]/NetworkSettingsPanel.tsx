import { Button, InfoRow, Panel, SECTION_THEMES } from "@teton/smith-ui";
import { Info, RefreshCw, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";
import {
	type Device,
	type DeviceSetting,
	type DeviceSettings,
	useGetSettingsForDevice,
	useIssueCommandsToDevices,
} from "@/app/api-client";
import { RelativeTime } from "@/app/components/RelativeTime";
import { Tooltip } from "@/app/components/tooltip";

const POLL_INTERVAL_MS = 30_000;
/** A requested report lands after the device's next poll plus its POST. */
const REFETCH_AFTER_REFRESH_MS = [5_000, 25_000];

/** The vendor driver's `scan_int` counts in 2 s units. */
const SCAN_INTERVAL_UNIT_S = 2;
/** Driver signal units are dBm + 100 on both vendor driver versions. */
const DRIVER_SIGNAL_DBM_OFFSET = 100;
/** `00` is the world regulatory domain: no country set. */
const NO_COUNTRY = "00";
/** NetworkManager `autoconnect-retries` value for "never give up". */
const RETRY_FOREVER = 0;

const MS_PER_S = 1_000;
const S_PER_MIN = 60;
const S_PER_HOUR = 3_600;

const useDeviceSettings = (serial: string) =>
	useGetSettingsForDevice(serial, {
		query: { refetchInterval: POLL_INTERVAL_MS },
	});

const useRefreshDeviceSettings = (
	device: Device | undefined,
	refetch: () => void,
) => {
	const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
	useEffect(
		() => () => {
			for (const timer of timers.current) clearTimeout(timer);
		},
		[],
	);
	const { mutate, isPending } = useIssueCommandsToDevices({
		mutation: {
			onSuccess: () => {
				for (const delay of REFETCH_AFTER_REFRESH_MS) {
					timers.current.push(setTimeout(refetch, delay));
				}
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
	return { refresh, isPending, canRefresh: device?.id !== undefined };
};

/** Renders one setting: "not used" when the distribution doesn't have it
 *  (wins over a read error, e.g. devices without the Realtek driver), "couldn't
 *  read" when the device failed to read it, otherwise the formatted value. */
const SettingValue = ({
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

/** Setting name with an info icon; the description shows on hover. Placed
 *  to the right because the panel clips anything left of the labels. */
const SettingLabel = ({ text, info }: { text: string; info: string }) => (
	<span className="inline-flex items-center gap-1">
		{text}
		<Tooltip content={info} placement="right">
			<Info className="h-3.5 w-3.5 text-gray-400" aria-label={info} />
		</Tooltip>
	</span>
);

interface Flag {
	bit: number;
	label: string;
}

// Bits from the driver's `rtw_wireless_mode` (include/ieee80211.h).
const MODE_BITS: Flag[] = [
	{ bit: 1, label: "802.11b" },
	{ bit: 2, label: "802.11g" },
	{ bit: 4, label: "802.11a" },
	{ bit: 8, label: "802.11n (2.4 GHz)" },
	{ bit: 16, label: "802.11n (5 GHz)" },
	{ bit: 64, label: "802.11ac" },
];
const OTHER_MODE_BITS: Record<number, string> = { 32: "AUTO" };

// Bits from the driver's `roam_flags` (include/rtw_mlme.h).
const ROAM_BITS: Flag[] = [
	{ bit: 1, label: "On link loss" },
	{ bit: 2, label: "On resume" },
	{ bit: 4, label: "Active (weak signal)" },
];

/** Read-only checkboxes for a driver bit field; set bits outside `flags` are
 *  listed below, by name when known. */
const DriverFlags = ({
	value,
	flags,
	otherNames = {},
}: {
	value: number;
	flags: Flag[];
	otherNames?: Record<number, string>;
}) => {
	const known = flags.reduce((acc, f) => acc | f.bit, 0);
	const other: string[] = [];
	for (let bit = 1; bit <= value; bit <<= 1) {
		if (value & bit && !(known & bit)) {
			other.push(otherNames[bit] ? `${otherNames[bit]} (${bit})` : `${bit}`);
		}
	}
	return (
		<div className="flex flex-col items-end gap-1">
			<div className="flex flex-wrap justify-end gap-x-3 gap-y-1">
				{flags.map((f) => (
					<label
						key={f.bit}
						className="inline-flex items-center gap-1 text-xs text-gray-700"
					>
						<input
							type="checkbox"
							checked={(value & f.bit) !== 0}
							disabled
							className="h-3.5 w-3.5"
						/>
						{f.label}
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

/** A value with a one-line explanation under it. */
const Described = ({ value, note }: { value: ReactNode; note?: string }) => (
	<span className="flex flex-col items-end">
		<span>{value}</span>
		{note && <span className="text-xs text-gray-500">{note}</span>}
	</span>
);

interface NamedValue {
	name: string;
	note?: string;
}

/** `rtw_power_mgnt` values (include/rtw_pwrctrl.h). */
const POWER_MGNT: Record<number, NamedValue> = {
	0: { name: "Active", note: "Radio never sleeps" },
	1: { name: "Min", note: "Light power saving" },
	2: { name: "Max", note: "Deepest power saving, slowest to wake" },
	3: { name: "DTIM", note: "Wakes at each DTIM beacon" },
	4: { name: "VoIP", note: "Power saving tuned for voice traffic" },
	5: { name: "UAPSD WMM", note: "Power saving driven by WMM traffic" },
};

/** NetworkManager `wifi.powersave` values. */
const NM_POWERSAVE: Record<number, NamedValue> = {
	0: { name: "Default", note: "Follows the global default" },
	1: { name: "Ignore", note: "Left to the driver's own power saving" },
	2: { name: "Disabled" },
	3: { name: "Enabled" },
};

/** Named value with its note, or the raw number when unknown. */
const NamedSetting = ({
	value,
	names,
}: {
	value: number;
	names: Record<number, NamedValue>;
}) => {
	const named = names[value];
	return named ? (
		<Described value={named.name} note={named.note} />
	) : (
		<>Unknown ({value})</>
	);
};

const describeClonedMac = (value: string) => {
	switch (value) {
		case "preserve":
			return "Keeps the MAC the interface has (normally the hardware MAC)";
		case "permanent":
			return "Always the hardware MAC";
		case "random":
			return "New random MAC on every connection";
		case "stable":
			return "Random MAC, fixed per network";
		default:
			return "Fixed MAC set in the configuration";
	}
};

const ServerList = ({ servers }: { servers?: string[] }) =>
	servers && servers.length > 0 ? (
		<span className="flex flex-col items-end font-mono text-xs">
			{servers.map((server) => (
				<span key={server}>{server}</span>
			))}
		</span>
	) : (
		<span className="text-gray-400">None</span>
	);

const threshold = (value: number) =>
	`${value} (≈ ${value - DRIVER_SIGNAL_DBM_OFFSET} dBm)`;

const duration = (seconds: number) => {
	if (seconds >= S_PER_HOUR && seconds % S_PER_HOUR === 0)
		return `${seconds / S_PER_HOUR} h`;
	if (seconds >= S_PER_MIN && seconds % S_PER_MIN === 0)
		return `${seconds / S_PER_MIN} min`;
	return `${seconds} s`;
};

/** `simple:<short s>:<signal dBm>:<long s>` from wpa_supplicant. */
const describeBgscan = (bgscan: string) => {
	const [module, short, signal, long] = bgscan.split(":");
	if (module !== "simple" || !short || !signal || !long) return null;
	return `Scans every ${duration(Number(short))} below ${signal} dBm, every ${duration(Number(long))} above`;
};

/** Settings the device reports it runs: Realtek driver, roaming,
 *  NetworkManager defaults, time sync and hostname. Read only; editing comes
 *  later. */
const NetworkSettingsPanel = ({
	serial,
	device,
}: {
	serial: string;
	device: Device | undefined;
}) => {
	const {
		data: settings,
		isLoading,
		isError,
		refetch,
	} = useDeviceSettings(serial);
	const { refresh, isPending, canRefresh } = useRefreshDeviceSettings(
		device,
		refetch,
	);
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
						disabled={!canRefresh}
						onClick={refresh}
						icon={<RefreshCw className="w-3.5 h-3.5" />}
					>
						Refresh
					</Button>
				</div>
			}
		>
			{isError ? (
				<p className="text-sm text-amber-600">Couldn't load settings.</p>
			) : isLoading || !settings ? (
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
						<InfoRow
							label={
								<SettingLabel
									text="Mode"
									info="802.11 standards the Wi-Fi driver allows"
								/>
							}
						>
							<SettingValue settings={settings} field="wireless_mode">
								{r.wireless_mode !== undefined && (
									<DriverFlags
										value={r.wireless_mode}
										flags={MODE_BITS}
										otherNames={OTHER_MODE_BITS}
									/>
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Power management"
									info="Realtek driver power saving level"
								/>
							}
						>
							<SettingValue settings={settings} field="power_mgnt">
								{r.power_mgnt !== undefined && (
									<NamedSetting value={r.power_mgnt} names={POWER_MGNT} />
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Country"
									info="Regulatory country: decides which channels are allowed"
								/>
							}
						>
							<SettingValue settings={settings} field="country_code">
								{r.country_code === NO_COUNTRY ? (
									<Described
										value="Unset"
										note="World domain: the most restrictive channel set"
									/>
								) : (
									r.country_code
								)}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Roaming
						</h5>
						<InfoRow
							label={
								<SettingLabel
									text="Driver roaming"
									info="When the Wi-Fi driver itself moves to another AP"
								/>
							}
						>
							<SettingValue settings={settings} field="roam_flags">
								{r.roam_flags !== undefined && (
									<DriverFlags value={r.roam_flags} flags={ROAM_BITS} />
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Idle RSSI diff"
									info="How much stronger another AP must be to roam (idle)"
								/>
							}
						>
							<SettingValue settings={settings} field="idle_diff">
								{r.idle_diff}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Busy RSSI diff"
									info="How much stronger another AP must be to roam (busy)"
								/>
							}
						>
							<SettingValue settings={settings} field="busy_diff">
								{r.busy_diff}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Scan result expiry"
									info="How old a scan result can be and still be a roam target"
								/>
							}
						>
							<SettingValue settings={settings} field="scanr_exp_ms">
								{r.scanr_exp_ms !== undefined &&
									`${r.scanr_exp_ms / MS_PER_S} s`}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Scan interval"
									info="Time between driver roaming scans while signal is weak"
								/>
							}
						>
							<SettingValue settings={settings} field="scan_int">
								{r.scan_int !== undefined &&
									`${r.scan_int} (${duration(r.scan_int * SCAN_INTERVAL_UNIT_S)})`}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Idle RSSI threshold"
									info="Signal below which the driver looks for a better AP (idle)"
								/>
							}
						>
							<SettingValue settings={settings} field="idle_th">
								{r.idle_th !== undefined && threshold(r.idle_th)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Busy RSSI threshold"
									info="Signal below which the driver looks for a better AP (busy)"
								/>
							}
						>
							<SettingValue settings={settings} field="busy_th">
								{r.busy_th !== undefined && threshold(r.busy_th)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Background scan (bgscan)"
									info="wpa_supplicant scans that can switch APs, apart from the driver"
								/>
							}
						>
							<SettingValue settings={settings} field="bgscan">
								{r.bgscan && (
									<Described
										value={
											<span className="font-mono text-xs">{r.bgscan}</span>
										}
										note={describeBgscan(r.bgscan) ?? undefined}
									/>
								)}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							NetworkManager defaults
						</h5>
						<InfoRow
							label={
								<SettingLabel
									text="Wi-Fi power save"
									info="NetworkManager Wi-Fi power saving default"
								/>
							}
						>
							<SettingValue settings={settings} field="nm_wifi_powersave">
								{r.nm_wifi_powersave !== undefined && (
									<NamedSetting
										value={r.nm_wifi_powersave}
										names={NM_POWERSAVE}
									/>
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Autoconnect retries"
									info="Tries before NetworkManager pauses reconnecting"
								/>
							}
						>
							<SettingValue settings={settings} field="nm_autoconnect_retries">
								{r.nm_autoconnect_retries !== undefined &&
									(r.nm_autoconnect_retries === RETRY_FOREVER ? (
										<Described value="Forever" note="Never stops retrying" />
									) : (
										<Described
											value={r.nm_autoconnect_retries}
											note={`Stops after ${r.nm_autoconnect_retries} failed attempts`}
										/>
									))}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Connection MAC"
									info="MAC address the device uses to connect"
								/>
							}
						>
							<SettingValue settings={settings} field="nm_cloned_mac">
								{r.nm_cloned_mac && (
									<Described
										value={
											<span className="font-mono text-xs">
												{r.nm_cloned_mac}
											</span>
										}
										note={describeClonedMac(r.nm_cloned_mac)}
									/>
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Scan MAC randomization"
									info="Random MAC in Wi-Fi scans (probe requests only)"
								/>
							}
						>
							<SettingValue settings={settings} field="nm_scan_rand_mac">
								{r.nm_scan_rand_mac !== undefined &&
									(r.nm_scan_rand_mac ? (
										<Described value="On" />
									) : (
										<Described value="Off" />
									))}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Time
						</h5>
						<InfoRow
							label={
								<SettingLabel
									text="NTP servers"
									info="Time servers configured on the device"
								/>
							}
						>
							<SettingValue settings={settings} field="ntp_servers">
								<ServerList servers={r.ntp_servers} />
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="From DHCP"
									info="Time servers handed out by the network"
								/>
							}
						>
							<SettingValue settings={settings} field="ntp_link_servers">
								<ServerList servers={r.ntp_link_servers} />
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Fallback servers"
									info="Used only when no other time server answers"
								/>
							}
						>
							<SettingValue settings={settings} field="ntp_fallback_servers">
								<ServerList servers={r.ntp_fallback_servers} />
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Server in use"
									info="Time server the clock currently follows"
								/>
							}
						>
							<SettingValue settings={settings} field="ntp_server_in_use">
								{r.ntp_server_in_use && (
									<span className="font-mono text-xs">
										{r.ntp_server_in_use}
									</span>
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="Clock synchronized"
									info="Whether the clock is in sync with a time server"
								/>
							}
						>
							<SettingValue settings={settings} field="ntp_synchronized">
								{r.ntp_synchronized !== undefined &&
									(r.ntp_synchronized ? "Yes" : "No")}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Identity
						</h5>
						<InfoRow
							label={
								<SettingLabel
									text="Hostname"
									info="Device name, also sent to the network's DHCP server"
								/>
							}
						>
							<SettingValue settings={settings} field="hostname">
								{r.hostname && <span className="font-mono">{r.hostname}</span>}
							</SettingValue>
						</InfoRow>
					</section>
				</div>
			)}
		</Panel>
	);
};

export default NetworkSettingsPanel;
