import { Badge, Button, InfoRow, Panel, SECTION_THEMES } from "@teton/smith-ui";
import { Info, Pencil, Play, RefreshCw, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import SettingsEditor from "@/app/(private)/devices/SettingsEditor";
import { ApplySettingsConfirmModal } from "@/app/(private)/devices/SettingsModals";
import {
	type Device,
	type DeviceSetting,
	type DeviceSettings,
	type ReportedSettings,
	type SettingsChanges,
	type SettingValues,
	useApplySettingsForDevice,
	useGetSettingsForDevice,
	useIssueCommandsToDevices,
	useUpdateSettingsForDevice,
} from "@/app/api-client";
import { RelativeTime } from "@/app/components/RelativeTime";
import { Tooltip } from "@/app/components/tooltip";
import { countryDiffersFromAp } from "@/app/utils/country";
import {
	applyAction,
	applyErrorMessage,
	type Flag,
	hasUnappliedEdits,
	hasWantedSettings,
	isL4t36,
	isSet,
	type ManagedField,
	MODE_BITS,
	type NamedValue,
	NM_POWERSAVE,
	OTHER_MODE_BITS,
	POWER_MGNT,
	ROAM_BITS,
	runningDiffers,
	runningValue,
	SETTING_LABEL,
	SOURCE_BADGE,
	STATE_BADGE,
	settingSource,
	settingState,
	settingsErrorMessage,
	wantedValue,
} from "@/app/utils/deviceSettings";

const POLL_INTERVAL_MS = 30_000;
/** The device answers a queued command on its next poll, so refetch at these
 *  delays rather than waiting for the regular interval. */
const REFETCH_AFTER_COMMAND_MS = [5_000, 25_000];

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

/** Fetched by device id, not serial: a numeric serial can match another
 *  device's id. Disabled until the device resolves. */
const useDeviceSettings = (device: Device | undefined) =>
	useGetSettingsForDevice(String(device?.id), {
		query: {
			enabled: device?.id !== undefined,
			refetchInterval: POLL_INTERVAL_MS,
		},
	});

/** Refetches after a queued command so its answer shows; timers are cleared on
 *  unmount. */
const useRefetchSoon = (refetch: () => void) => {
	const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
	useEffect(
		() => () => {
			for (const timer of timers.current) clearTimeout(timer);
		},
		[],
	);
	return () => {
		for (const delay of REFETCH_AFTER_COMMAND_MS) {
			timers.current.push(setTimeout(refetch, delay));
		}
	};
};

const useRefreshDeviceSettings = (
	device: Device | undefined,
	refetch: () => void,
) => {
	const refetchSoon = useRefetchSoon(refetch);
	const { mutate, isPending } = useIssueCommandsToDevices({
		mutation: {
			onSuccess: refetchSoon,
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

/** Saves pins by device id, then refetches so the badges and the unapplied
 *  edits banner update. */
const useSaveDeviceSettings = (
	device: Device | undefined,
	refetch: () => Promise<unknown>,
	onSaved: () => void,
) => {
	const { mutate, isPending, error, reset } = useUpdateSettingsForDevice({
		mutation: {
			onSuccess: async () => {
				await refetch();
				onSaved();
			},
		},
	});
	const save = (changes: SettingsChanges) => {
		if (device?.id === undefined) return;
		mutate({ deviceId: String(device.id), data: changes });
	};
	return {
		save,
		isSaving: isPending,
		saveError: error ? settingsErrorMessage(error) : undefined,
		resetError: reset,
	};
};

/** Queues ApplySettings by device id, then refetches now and at the delayed
 *  intervals so Pending and then the result show up. */
const useApplyDeviceSettings = (
	device: Device | undefined,
	refetch: () => void,
	onQueued: () => void,
) => {
	const refetchSoon = useRefetchSoon(refetch);
	const { mutate, isPending, error, reset } = useApplySettingsForDevice({
		mutation: {
			onSuccess: () => {
				refetch();
				refetchSoon();
				onQueued();
			},
		},
	});
	const apply = () => {
		if (device?.id === undefined) return;
		mutate({ deviceId: String(device.id) });
	};
	return {
		apply,
		isApplying: isPending,
		applyError: error ? applyErrorMessage(error) : undefined,
		resetApplyError: reset,
	};
};

/** Renders a setting's value, or in its place why there is none: "not used"
 *  when the distribution doesn't have it (wins over a read error, e.g. devices
 *  without the Realtek driver), "not reported", "couldn't read", or "none". */
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
		return <span className="text-gray-400">{STATE_BADGE.notUsed.label}</span>;
	}
	if (!settings.reported) {
		return <span className="text-gray-400">Not reported</span>;
	}
	if (settings.reported.read_errors?.includes(field)) {
		return (
			<span className="text-amber-600">{STATE_BADGE.unreadable.label}</span>
		);
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

/** A setting row: the wanted value (or, when unmanaged, what the device runs)
 *  with its source and state badges, plus a "Running:" line only when the
 *  device runs something else. `render` formats a value. */
const ManagedRow = <K extends ManagedField>({
	settings,
	field,
	info,
	render = (value) => value as ReactNode,
	note,
}: {
	settings: DeviceSettings;
	field: K;
	info: string;
	render?: (value: NonNullable<SettingValues[K]>) => ReactNode;
	note?: string;
}) => {
	const wanted = wantedValue(settings, field);
	const running = runningValue(settings, field);
	const source = settingSource(settings, field);
	const status = settingState(settings, field);
	const label = <SettingLabel text={SETTING_LABEL[field]} info={info} />;
	if (status?.state === "notUsed") {
		return (
			<InfoRow label={label}>
				<SettingValue settings={settings} field={field}>
					{null}
				</SettingValue>
			</InfoRow>
		);
	}
	const sourceBadge = source
		? SOURCE_BADGE[source]
		: { variant: "gray" as const, label: "Not managed" };
	return (
		<InfoRow label={label}>
			<span className="flex flex-col items-end gap-1">
				{wanted !== undefined ? (
					render(wanted)
				) : (
					<SettingValue settings={settings} field={field}>
						{running !== undefined && render(running)}
					</SettingValue>
				)}
				<span className="flex flex-wrap items-center justify-end gap-1">
					<Badge variant={sourceBadge.variant}>{sourceBadge.label}</Badge>
					{status && (
						<Badge variant={STATE_BADGE[status.state].variant}>
							{STATE_BADGE[status.state].label}
						</Badge>
					)}
				</span>
				{status?.reason && (
					<span className="text-xs text-red-600">{status.reason}</span>
				)}
				{settings.reported &&
					!settings.reported.read_errors?.includes(field) &&
					runningDiffers(settings, field) && (
						<span className="flex flex-col items-end text-xs text-gray-500">
							Running: {running === undefined ? "None" : render(running)}
						</span>
					)}
				{note && <span className="text-xs text-gray-500">{note}</span>}
			</span>
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
 *  NetworkManager defaults, time sync and hostname, with an editor for the pins. */
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
	const [editing, setEditing] = useState(false);
	const { save, isSaving, saveError, resetError } = useSaveDeviceSettings(
		device,
		refetch,
		() => setEditing(false),
	);
	const [confirmingApply, setConfirmingApply] = useState(false);
	const { apply, isApplying, applyError, resetApplyError } =
		useApplyDeviceSettings(device, refetch, () => setConfirmingApply(false));
	const closeApplyConfirm = () => {
		resetApplyError();
		setConfirmingApply(false);
	};
	const applyState = settings ? applyAction(settings) : undefined;
	const reported = settings?.reported;
	// Without a report `r` is empty, which is safe: `SettingValue` shows "Not
	// reported" before rendering any of it.
	const r: Partial<ReportedSettings> = reported ?? {};

	return (
		<Panel
			title="Network Settings"
			icon={SlidersHorizontal}
			theme={SECTION_THEMES.purple}
			actions={
				<div className="flex items-center gap-2">
					{reported && (
						<span className="text-xs text-gray-500">
							Reported <RelativeTime date={reported.reported_at} />
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
					{settings && !editing && (
						<Button
							variant="soft"
							size="sm"
							onClick={() => setEditing(true)}
							icon={<Pencil className="w-3.5 h-3.5" />}
						>
							Edit
						</Button>
					)}
					{applyState && (
						<Button
							variant="soft"
							size="sm"
							disabled={editing || !applyState.canApply}
							onClick={() => setConfirmingApply(true)}
							icon={<Play className="w-3.5 h-3.5" />}
						>
							{applyState.label}
						</Button>
					)}
				</div>
			}
		>
			{isError ? (
				<p className="text-sm text-amber-600">Couldn't load settings.</p>
			) : isLoading || !settings ? (
				<div className="h-4 animate-pulse rounded bg-gray-100" />
			) : editing ? (
				<SettingsEditor
					resolved={settings.resolved}
					defaults={settings.defaults}
					unsupported={settings.unsupported}
					wanted={settings.wanted}
					saving={isSaving}
					error={saveError}
					onSave={save}
					onCancel={() => {
						resetError();
						setEditing(false);
					}}
				/>
			) : !reported && !hasWantedSettings(settings) ? (
				<p className="text-sm text-gray-500">
					Not reported. The device needs a smithd version that reports its
					settings.
				</p>
			) : (
				<div className="space-y-5">
					{!reported && (
						<p className="text-xs text-gray-500">
							Not reported yet: showing the wanted values only.
						</p>
					)}
					{hasUnappliedEdits(settings) && (
						<p className="rounded bg-yellow-50 px-3 py-2 text-xs text-yellow-800">
							Edits not applied yet: settings version{" "}
							{settings.settings_version}, the device applied{" "}
							{r.applied_version ?? "none"}.
						</p>
					)}
					<section className="space-y-2">
						<h5 className="text-xs font-semibold uppercase text-gray-500">
							Driver
						</h5>
						<ManagedRow
							settings={settings}
							field="wireless_mode"
							info="802.11 standards the Wi-Fi driver allows"
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
							info="Realtek driver power saving level"
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
						<InfoRow
							label={
								<SettingLabel
									text="AP country"
									info="Country the connected access point announces; the device's country should match it"
								/>
							}
						>
							<SettingValue settings={settings} field="ap_country_code">
								{isSet(r.ap_country_code) &&
									(countryDiffersFromAp(r.country_code, r.ap_country_code) ? (
										<span className="flex flex-col items-end">
											<span>{r.ap_country_code}</span>
											<span className="text-xs text-amber-600">
												Differs from the device's country
											</span>
										</span>
									) : (
										r.ap_country_code
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
							info="When the Wi-Fi driver itself moves to another AP"
							render={(v) => <DriverFlags value={v} flags={ROAM_BITS} />}
						/>
						<ManagedRow
							settings={settings}
							field="idle_diff"
							info="How much stronger another AP must be to roam (idle)"
						/>
						<ManagedRow
							settings={settings}
							field="busy_diff"
							info="How much stronger another AP must be to roam (busy)"
						/>
						<ManagedRow
							settings={settings}
							field="scanr_exp_ms"
							info="How old a scan result can be and still be a roam target"
							render={(v) => `${v / MS_PER_S} s`}
						/>
						<ManagedRow
							settings={settings}
							field="scan_int"
							info="Time between driver roaming scans while signal is weak"
							render={(v) => `${v} (${duration(v * SCAN_INTERVAL_UNIT_S)})`}
						/>
						<ManagedRow
							settings={settings}
							field="idle_th"
							info="Signal below which the driver looks for a better AP (idle)"
							render={threshold}
						/>
						<ManagedRow
							settings={settings}
							field="busy_th"
							info="Signal below which the driver looks for a better AP (busy)"
							render={threshold}
						/>
						<ManagedRow
							settings={settings}
							field="bgscan"
							info="wpa_supplicant scans that can switch APs, apart from the driver"
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
							info="NetworkManager Wi-Fi power saving default"
							render={(v) => <NamedSetting value={v} names={NM_POWERSAVE} />}
						/>
						<ManagedRow
							settings={settings}
							field="nm_autoconnect_retries"
							info="Tries before NetworkManager pauses reconnecting"
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
							info="MAC address the device uses to connect"
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
							info="Random MAC in Wi-Fi scans (probe requests only)"
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
							info="Time servers configured on the device"
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
								{isSet(r.ntp_synchronized) &&
									(r.ntp_synchronized ? "Yes" : "No")}
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
							info="Device name, also sent to the network's DHCP server"
							render={(v) => <span className="font-mono">{v}</span>}
						/>
					</section>
				</div>
			)}
			<ApplySettingsConfirmModal
				open={confirmingApply}
				label={applyState?.label ?? "Apply"}
				pending={isApplying}
				error={applyError}
				onConfirm={apply}
				onClose={closeApplyConfirm}
			/>
		</Panel>
	);
};

export default NetworkSettingsPanel;
