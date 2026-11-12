import {
	Badge,
	type BadgeVariant,
	Button,
	InfoRow,
	Panel,
	SECTION_THEMES,
} from "@teton/smith-ui";
import { Info, RefreshCw, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";
import {
	type Device,
	type DeviceSetting,
	type DeviceSettings,
	type SettingValues,
	useGetSettingsForDevice,
	useIssueCommandsToDevices,
} from "@/app/api-client";
import { RelativeTime } from "@/app/components/RelativeTime";
import { Tooltip } from "@/app/components/tooltip";
import { countryDiffersFromAp } from "@/app/utils/country";
import {
	hasUnappliedEdits,
	hasWantedSettings,
	isL4t36,
	type ManagedField,
	runningDiffers,
	type SettingState,
	SOURCE_LABEL,
	STATE_LABEL,
	settingSource,
	settingState,
	wantedValue,
} from "@/app/utils/deviceSettings";

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
const BGSCAN_L4T36_NOTE = "No effect on L4T 36: the driver does the roaming";

const MS_PER_S = 1_000;
const S_PER_MIN = 60;
const S_PER_HOUR = 3_600;

/** Called by device id, not serial: a numeric serial can match another
 *  device's id (ADR 0006). Waits for the device to resolve. */
const useDeviceSettings = (device: Device | undefined) =>
	useGetSettingsForDevice(String(device?.id), {
		query: {
			enabled: device?.id !== undefined,
			refetchInterval: POLL_INTERVAL_MS,
		},
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
	if (!settings.reported) {
		return <span className="text-gray-400">Not reported</span>;
	}
	if (settings.reported.read_errors?.includes(field)) {
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

const STATE_VARIANT: Record<SettingState, BadgeVariant> = {
	notUsed: "gray",
	unreadable: "yellow",
	failed: "red",
	reverted: "red",
	pending: "yellow",
	differs: "orange",
	inSync: "green",
};

/** A managed setting: the wanted value with its Default or Pinned badge and
 *  state; a "Running:" line only when the device runs something else. Not
 *  managed settings show what the device runs. `render` formats a value,
 *  wanted or running. */
const ManagedRow = <K extends ManagedField & DeviceSetting>({
	settings,
	field,
	label,
	render,
	note,
}: {
	settings: DeviceSettings;
	field: K;
	label: ReactNode;
	render: (value: NonNullable<SettingValues[K]>) => ReactNode;
	note?: string;
}) => {
	const wanted = wantedValue(settings, field);
	const running = settings.reported?.[field];
	const source = settingSource(settings, field);
	const status = settingState(settings, field);
	const shown = wanted ?? running;
	const unusable = status?.state === "notUsed";
	return (
		<InfoRow label={label}>
			{unusable ? (
				<span className="text-gray-400">{STATE_LABEL.notUsed}</span>
			) : (
				<span className="flex flex-col items-end gap-1">
					{status?.state === "unreadable" && wanted === undefined ? (
						<span className="text-amber-600">{STATE_LABEL.unreadable}</span>
					) : shown === undefined ? (
						<span className="text-gray-400">
							{settings.reported ? "None" : "Not reported"}
						</span>
					) : (
						render(shown as NonNullable<SettingValues[K]>)
					)}
					<span className="flex flex-wrap items-center justify-end gap-1">
						{source ? (
							<Badge variant={source === "default" ? "gray" : "purple"}>
								{SOURCE_LABEL[source]}
							</Badge>
						) : (
							<Badge variant="gray">Not managed</Badge>
						)}
						{status &&
							!(status.state === "unreadable" && wanted === undefined) && (
								<Badge variant={STATE_VARIANT[status.state]}>
									{STATE_LABEL[status.state]}
								</Badge>
							)}
					</span>
					{status?.reason && (
						<span className="text-xs text-red-600">{status.reason}</span>
					)}
					{wanted !== undefined &&
						settings.reported &&
						status?.state !== "unreadable" &&
						runningDiffers(settings, field) && (
							<span className="flex flex-col items-end text-xs text-gray-500">
								Running:{" "}
								{running === undefined
									? "None"
									: render(running as NonNullable<SettingValues[K]>)}
							</span>
						)}
					{note && <span className="text-xs text-gray-500">{note}</span>}
				</span>
			)}
		</InfoRow>
	);
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

/** Wanted settings next to what the device runs: Realtek driver, roaming,
 *  NetworkManager defaults, time sync and hostname. Read only; editing comes
 *  later. */
const NetworkSettingsPanel = ({ device }: { device: Device | undefined }) => {
	const {
		data: settings,
		isLoading,
		isError,
		refetch,
	} = useDeviceSettings(device);
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
			) : !r && !hasWantedSettings(settings) ? (
				<p className="text-sm text-gray-500">
					Not reported. The device needs a smithd version that reports its
					settings.
				</p>
			) : (
				<div className="space-y-5">
					{!r && (
						<p className="text-xs text-gray-500">
							Not reported yet: showing the wanted values only.
						</p>
					)}
					{hasUnappliedEdits(settings) && (
						<p className="rounded bg-yellow-50 px-3 py-2 text-xs text-yellow-800">
							Edits not applied yet: settings version{" "}
							{settings.settings_version}, the device applied{" "}
							{r?.applied_version ?? "none"}.
						</p>
					)}
					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Driver
						</h5>
						<ManagedRow
							settings={settings}
							field="wireless_mode"
							label={
								<SettingLabel
									text="Mode"
									info="802.11 standards the Wi-Fi driver allows"
								/>
							}
							render={(v) => (
								<DriverFlags
									value={v}
									flags={MODE_BITS}
									otherNames={OTHER_MODE_BITS}
								/>
							)}
						/>
						<ManagedRow
							settings={settings}
							field="power_mgnt"
							label={
								<SettingLabel
									text="Power management"
									info="Realtek driver power saving level"
								/>
							}
							render={(v) => <NamedSetting value={v} names={POWER_MGNT} />}
						/>
						<InfoRow
							label={
								<SettingLabel
									text="Country"
									info="Regulatory country: decides which channels are allowed"
								/>
							}
						>
							<SettingValue settings={settings} field="country_code">
								{r?.country_code === NO_COUNTRY ? (
									<Described
										value="Unset"
										note="World domain: the most restrictive channel set"
									/>
								) : (
									r?.country_code
								)}
							</SettingValue>
						</InfoRow>
						<InfoRow
							label={
								<SettingLabel
									text="AP country"
									info="Country the connected access point announces; the device's country should match it"
								/>
							}
						>
							<SettingValue settings={settings} field="ap_country_code">
								{r?.ap_country_code !== undefined &&
									(countryDiffersFromAp(r?.country_code, r?.ap_country_code) ? (
										<span className="flex flex-col items-end">
											<span>{r?.ap_country_code}</span>
											<span className="text-xs text-amber-600">
												Differs from the device's country
											</span>
										</span>
									) : (
										r?.ap_country_code
									))}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Roaming
						</h5>
						<ManagedRow
							settings={settings}
							field="roam_flags"
							label={
								<SettingLabel
									text="Driver roaming"
									info="When the Wi-Fi driver itself moves to another AP"
								/>
							}
							render={(v) => <DriverFlags value={v} flags={ROAM_BITS} />}
						/>
						<ManagedRow
							settings={settings}
							field="idle_diff"
							label={
								<SettingLabel
									text="Idle RSSI diff"
									info="How much stronger another AP must be to roam (idle)"
								/>
							}
							render={(v) => v}
						/>
						<ManagedRow
							settings={settings}
							field="busy_diff"
							label={
								<SettingLabel
									text="Busy RSSI diff"
									info="How much stronger another AP must be to roam (busy)"
								/>
							}
							render={(v) => v}
						/>
						<ManagedRow
							settings={settings}
							field="scanr_exp_ms"
							label={
								<SettingLabel
									text="Scan result expiry"
									info="How old a scan result can be and still be a roam target"
								/>
							}
							render={(v) => `${v / MS_PER_S} s`}
						/>
						<ManagedRow
							settings={settings}
							field="scan_int"
							label={
								<SettingLabel
									text="Scan interval"
									info="Time between driver roaming scans while signal is weak"
								/>
							}
							render={(v) => `${v} (${duration(v * SCAN_INTERVAL_UNIT_S)})`}
						/>
						<ManagedRow
							settings={settings}
							field="idle_th"
							label={
								<SettingLabel
									text="Idle RSSI threshold"
									info="Signal below which the driver looks for a better AP (idle)"
								/>
							}
							render={threshold}
						/>
						<ManagedRow
							settings={settings}
							field="busy_th"
							label={
								<SettingLabel
									text="Busy RSSI threshold"
									info="Signal below which the driver looks for a better AP (busy)"
								/>
							}
							render={threshold}
						/>
						<ManagedRow
							settings={settings}
							field="bgscan"
							label={
								<SettingLabel
									text="Background scan (bgscan)"
									info="wpa_supplicant scans that can switch APs, apart from the driver"
								/>
							}
							render={(v) => (
								<Described
									value={<span className="font-mono text-xs">{v}</span>}
									note={describeBgscan(v) ?? undefined}
								/>
							)}
							note={
								isL4t36(device?.release?.distribution_name)
									? BGSCAN_L4T36_NOTE
									: undefined
							}
						/>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							NetworkManager defaults
						</h5>
						<ManagedRow
							settings={settings}
							field="nm_wifi_powersave"
							label={
								<SettingLabel
									text="Wi-Fi power save"
									info="NetworkManager Wi-Fi power saving default"
								/>
							}
							render={(v) => <NamedSetting value={v} names={NM_POWERSAVE} />}
						/>
						<ManagedRow
							settings={settings}
							field="nm_autoconnect_retries"
							label={
								<SettingLabel
									text="Autoconnect retries"
									info="Tries before NetworkManager pauses reconnecting"
								/>
							}
							render={(v) =>
								v === RETRY_FOREVER ? (
									<Described value="Forever" note="Never stops retrying" />
								) : (
									<Described
										value={v}
										note={`Stops after ${v} failed attempts`}
									/>
								)
							}
						/>
						<ManagedRow
							settings={settings}
							field="nm_cloned_mac"
							label={
								<SettingLabel
									text="Connection MAC"
									info="MAC address the device uses to connect"
								/>
							}
							render={(v) => (
								<Described
									value={<span className="font-mono text-xs">{v}</span>}
									note={describeClonedMac(v)}
								/>
							)}
						/>
						<ManagedRow
							settings={settings}
							field="nm_scan_rand_mac"
							label={
								<SettingLabel
									text="Scan MAC randomization"
									info="Random MAC in Wi-Fi scans (probe requests only)"
								/>
							}
							render={(v) => <Described value={v ? "On" : "Off"} />}
						/>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Time
						</h5>
						<ManagedRow
							settings={settings}
							field="ntp_servers"
							label={
								<SettingLabel
									text="NTP servers"
									info="Time servers configured on the device"
								/>
							}
							render={(v) => <ServerList servers={v} />}
						/>
						<InfoRow
							label={
								<SettingLabel
									text="From DHCP"
									info="Time servers handed out by the network"
								/>
							}
						>
							<SettingValue settings={settings} field="ntp_link_servers">
								<ServerList servers={r?.ntp_link_servers} />
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
								<ServerList servers={r?.ntp_fallback_servers} />
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
								{r?.ntp_server_in_use && (
									<span className="font-mono text-xs">
										{r?.ntp_server_in_use}
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
								{r?.ntp_synchronized !== undefined &&
									(r?.ntp_synchronized ? "Yes" : "No")}
							</SettingValue>
						</InfoRow>
					</section>

					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Identity
						</h5>
						<ManagedRow
							settings={settings}
							field="hostname"
							label={
								<SettingLabel
									text="Hostname"
									info="Device name, also sent to the network's DHCP server"
								/>
							}
							render={(v) => <span className="font-mono">{v}</span>}
						/>
					</section>
				</div>
			)}
		</Panel>
	);
};

export default NetworkSettingsPanel;
