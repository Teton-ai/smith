import { Button, Select } from "@teton/smith-ui";
import { type ReactNode, useState } from "react";
import type {
	DeviceSettings,
	SettingsChanges,
	SettingValues,
} from "@/app/api-client";
import {
	AUTOCONNECT_RETRIES_OPTIONS,
	defaultLabel,
	draftToChange,
	type EditDraft,
	type EditGroup,
	type EditGroupKey,
	editGroups,
	type Flag,
	groupsToReset,
	initialDraft,
	MODE_BITS,
	type NamedOption,
	NM_POWERSAVE_OPTIONS,
	optionsWith,
	type PinFields,
	type PinValue,
	POWER_MGNT_OPTIONS,
	pinSeed,
	ROAM_BITS,
	roamTuningFields,
	SETTING_LABEL,
	sameDraft,
} from "@/app/utils/deviceSettings";

const NOT_SUPPORTED = "Not supported on this distribution";

/** Driver units shown next to the roam tuning inputs. */
const FIELD_UNIT: Partial<Record<keyof SettingValues, string>> = {
	scanr_exp_ms: " (ms)",
	scan_int: " (2 s units)",
};

const NAMED_OPTIONS: Partial<Record<EditGroupKey, NamedOption[]>> = {
	power_mgnt: POWER_MGNT_OPTIONS,
	nm_wifi_powersave: NM_POWERSAVE_OPTIONS,
	nm_autoconnect_retries: AUTOCONNECT_RETRIES_OPTIONS,
};

const FLAGS: Partial<Record<EditGroupKey, Flag[]>> = {
	wireless_mode: MODE_BITS,
	roam_flags: ROAM_BITS,
};

const INPUT_CLASS =
	"w-full rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-900 focus:border-blue-500 focus:ring-2 focus:ring-blue-500";

const FlagBoxes = ({
	flags,
	value,
	onChange,
}: {
	flags: Flag[];
	value: number;
	onChange: (value: number) => void;
}) => (
	<div className="flex flex-wrap gap-x-4 gap-y-1">
		{flags.map((f) => (
			<label
				key={f.bit}
				className="inline-flex items-center gap-1.5 text-sm text-gray-700"
			>
				<input
					type="checkbox"
					checked={(value & f.bit) !== 0}
					// XOR keeps driver bits the editor has no checkbox for.
					onChange={() => onChange(value ^ f.bit)}
					className="h-4 w-4"
				/>
				{f.label}
			</label>
		))}
	</div>
);

const PinControl = ({
	group,
	value,
	unsupported,
	onChange,
}: {
	group: EditGroup;
	value: PinValue;
	unsupported: DeviceSettings["unsupported"];
	onChange: (value: PinValue) => void;
}) => {
	const flags = FLAGS[group.key];
	if (flags) {
		return (
			<FlagBoxes flags={flags} value={Number(value)} onChange={onChange} />
		);
	}
	const options = NAMED_OPTIONS[group.key];
	if (options) {
		return (
			<Select
				value={String(value)}
				onChange={(v) => onChange(Number(v))}
				className="w-full"
			>
				{optionsWith(options, Number(value)).map((o) => (
					<option key={o.value} value={o.value}>
						{o.note ? `${o.name}: ${o.note}` : o.name}
					</option>
				))}
			</Select>
		);
	}
	if (group.key === "roam_tuning") {
		const fields = value as PinFields;
		return (
			<div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
				{roamTuningFields(unsupported).map((f) => (
					<label key={f} className="text-xs text-gray-600">
						{SETTING_LABEL[f as keyof typeof SETTING_LABEL]}
						{FIELD_UNIT[f] ?? ""}
						<input
							type="number"
							inputMode="numeric"
							value={String(fields[f] ?? "")}
							onChange={(e) => onChange({ ...fields, [f]: e.target.value })}
							className={INPUT_CLASS}
						/>
					</label>
				))}
			</div>
		);
	}
	if (group.key === "mac") {
		const fields = value as PinFields;
		return (
			<div className="space-y-2">
				<label className="block text-xs text-gray-600">
					Connection MAC (preserve, permanent, random, stable or an address)
					<input
						type="text"
						value={String(fields.cloned_mac)}
						onChange={(e) =>
							onChange({ ...fields, cloned_mac: e.target.value })
						}
						className={`${INPUT_CLASS} font-mono`}
					/>
				</label>
				<label className="inline-flex items-center gap-1.5 text-sm text-gray-700">
					<input
						type="checkbox"
						checked={fields.scan_rand_mac === true}
						onChange={(e) =>
							onChange({ ...fields, scan_rand_mac: e.target.checked })
						}
						className="h-4 w-4"
					/>
					Randomize the MAC in Wi-Fi scans
				</label>
			</div>
		);
	}
	if (group.key === "ntp_servers") {
		return (
			<textarea
				aria-label={group.label}
				rows={3}
				placeholder="One server per line, or comma separated"
				value={String(value)}
				onChange={(e) => onChange(e.target.value)}
				className={`${INPUT_CLASS} font-mono`}
			/>
		);
	}
	return (
		<input
			type="text"
			aria-label={group.label}
			value={String(value)}
			onChange={(e) => onChange(e.target.value)}
			className={`${INPUT_CLASS} font-mono`}
		/>
	);
};

const GroupRow = ({
	group,
	defaults,
	unsupported,
	draft,
	touched,
	bulk,
	onDefault,
	onPin,
	onChange,
	onUndo,
}: {
	group: EditGroup;
	defaults: SettingValues;
	unsupported: DeviceSettings["unsupported"];
	draft: EditDraft;
	touched: boolean;
	bulk: boolean;
	onDefault: () => void;
	onPin: () => void;
	onChange: (value: PinValue) => void;
	onUndo: () => void;
}) => {
	const name = `settings-edit-${group.key}`;
	return (
		<section className="space-y-2 border-b border-gray-100 pb-4 last:border-0">
			<div className="flex items-center justify-between gap-2">
				<h5 className="text-sm font-medium text-gray-900">{group.label}</h5>
				{group.supported &&
					(touched ? (
						<span className="flex items-center gap-2 text-xs text-blue-600">
							Will change
							<Button variant="ghost" tone="gray" size="sm" onClick={onUndo}>
								Undo
							</Button>
						</span>
					) : (
						bulk && <span className="text-xs text-gray-400">Unchanged</span>
					))}
			</div>
			{group.supported ? (
				<>
					<label className="flex items-center gap-2 text-sm text-gray-700">
						<input
							type="radio"
							name={name}
							checked={draft.mode === "default"}
							onChange={onDefault}
						/>
						{defaultLabel(group, defaults)}
					</label>
					<label className="flex items-center gap-2 text-sm text-gray-700">
						<input
							type="radio"
							name={name}
							checked={draft.mode === "pin"}
							onChange={onPin}
						/>
						Pin a value
					</label>
					{draft.mode === "pin" && (
						<div className="pl-6">
							<PinControl
								group={group}
								value={draft.value}
								unsupported={unsupported}
								onChange={onChange}
							/>
						</div>
					)}
				</>
			) : (
				<p className="text-sm text-gray-400">{NOT_SUPPORTED}</p>
			)}
		</section>
	);
};

/**
 * Edit form for one device's managed settings, or with `bulk` for several on one
 * distribution. Only touched groups are sent, so the others keep their pins.
 * Bulk mode leaves `wanted` out because the selection's pins are unknown.
 */
const SettingsEditor = ({
	resolved,
	defaults,
	unsupported,
	wanted = {},
	bulk = false,
	saving = false,
	error,
	onSave,
	onCancel,
}: {
	resolved: DeviceSettings["resolved"];
	defaults: SettingValues;
	unsupported: DeviceSettings["unsupported"];
	wanted?: SettingValues;
	bulk?: boolean;
	saving?: boolean;
	error?: ReactNode;
	onSave: (changes: SettingsChanges) => void;
	onCancel: () => void;
}) => {
	const groups = editGroups(resolved, bulk);
	const [edits, setEdits] = useState<Partial<Record<EditGroupKey, EditDraft>>>(
		{},
	);
	const [invalid, setInvalid] = useState<string[]>([]);

	const start = (key: EditGroupKey): EditDraft =>
		bulk ? { mode: "default" } : initialDraft(key, wanted);
	const current = (key: EditGroupKey) => edits[key] ?? start(key);

	const clearEdit = (key: EditGroupKey) =>
		setEdits((prev) => {
			const next = { ...prev };
			delete next[key];
			return next;
		});

	const setDraft = (key: EditGroupKey, draft: EditDraft) => {
		setInvalid([]);
		// Single device only: returning to the starting value drops the edit. Bulk
		// keeps it, since the selection's starting pins are unknown.
		if (!bulk && sameDraft(draft, start(key))) clearEdit(key);
		else setEdits((prev) => ({ ...prev, [key]: draft }));
	};

	const resetAll = () => {
		setInvalid([]);
		setEdits(
			Object.fromEntries(
				groupsToReset(groups, wanted, bulk).map((key) => [
					key,
					{ mode: "default" },
				]),
			),
		);
	};

	const save = () => {
		const changes: Partial<Record<EditGroupKey, unknown>> = {};
		const errors: string[] = [];
		for (const [key, draft] of Object.entries(edits) as [
			EditGroupKey,
			EditDraft,
		][]) {
			const result = draftToChange(key, draft, unsupported);
			if ("error" in result) errors.push(result.error);
			else changes[key] = result.change;
		}
		setInvalid(errors);
		if (errors.length === 0) {
			onSave(changes as SettingsChanges);
		}
	};

	const touchedCount = Object.keys(edits).length;
	return (
		<div className="space-y-4">
			{groups.map((group) => (
				<GroupRow
					key={group.key}
					group={group}
					defaults={defaults}
					unsupported={unsupported}
					draft={current(group.key)}
					touched={group.key in edits}
					bulk={bulk}
					onDefault={() => setDraft(group.key, { mode: "default" })}
					onPin={() =>
						setDraft(group.key, {
							mode: "pin",
							value: pinSeed(group.key, wanted, defaults),
						})
					}
					onChange={(value) => setDraft(group.key, { mode: "pin", value })}
					onUndo={() => clearEdit(group.key)}
				/>
			))}
			{invalid.length > 0 && (
				<ul className="text-sm text-red-600">
					{invalid.map((message) => (
						<li key={message}>{message}</li>
					))}
				</ul>
			)}
			{error && <p className="text-sm text-red-600">{error}</p>}
			<div className="flex items-center justify-between gap-2">
				<Button variant="ghost" tone="gray" size="sm" onClick={resetAll}>
					Reset all to default
				</Button>
				<div className="flex gap-2">
					<Button variant="soft" tone="gray" size="sm" onClick={onCancel}>
						Cancel
					</Button>
					<Button
						variant="solid"
						size="sm"
						loading={saving}
						disabled={touchedCount === 0}
						onClick={save}
					>
						Save
					</Button>
				</div>
			</div>
		</div>
	);
};

export default SettingsEditor;
