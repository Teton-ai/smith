---
title: Device settings
description: Choose how a device's Wi-Fi driver, NetworkManager, time sync and hostname are configured, from distribution defaults or per-device pins, and apply them safely.
---

The **Network** tab of a device shows a settings panel for the device's Wi-Fi driver, NetworkManager, NTP servers and hostname. Each setting has a value Smith wants the device to run, and the panel compares it with what the device reports.

Nothing you edit reaches the device until you press **Apply**.

## Where a value comes from

For each setting, Smith resolves the wanted value in this order:

1. The **pin** set on the device, if any.
2. Otherwise the **distribution default**, the value set for every device on the same distribution.
3. Otherwise the setting is **not managed**: Smith asks nothing of the device for it.

The badge next to each setting shows which one applies.

| Badge | Meaning |
| --- | --- |
| **Default** | The distribution default is used. |
| **Pinned** | A value is pinned on this device. |
| **Pinned (same as default)** | A value is pinned, and it equals the current default. A later change of the default does not move this device. |
| **Not managed** | No pin and no default. |

Distribution defaults have no editor in the dashboard. You can only pin values per device (or for a selection of devices, see [Bulk edit](#bulk-edit)).

## Managed settings and groups

| Setting | Notes |
| --- | --- |
| **Driver mode** | 802.11 standards the driver allows. |
| **Power management** | Driver power saving level. |
| **Driver roaming** | When the driver roams: on link loss, on resume, or when active with a weak signal. |
| **Roam tuning** | Idle and busy RSSI differences and thresholds, scan interval, scan result expiry. |
| **Wi-Fi power save** | NetworkManager's power save mode for the connection. |
| **Autoconnect retries** | `0` retries forever; `-1` uses NetworkManager's own default. |
| **MAC randomization** | The connection MAC and scan MAC randomization. |
| **NTP servers** | The time servers the device syncs with. |
| **Hostname** | The device's hostname. |
| **Background scan (bgscan)** | The supplicant's roaming scan. It has no effect on L4T 36, where the driver roams. |

A setting made of several values is a **group** and is always resolved as a whole. Roam tuning and MAC randomization are one group each: pinning a group replaces every value in it, and the values of a pin are never mixed with those of the default. A group set back to its default takes the whole default.

The closed choices (driver mode, power management, Wi-Fi power save, autoconnect retries) are shown by name in the editor.

### Settings a distribution does not support

Some distributions do not have some settings, for example the MAC settings on some L4T 35 distributions. The panel shows them as **Not used on this device**, and the editor shows **Not supported on this distribution** instead of a control. Smith leaves them out of what it sends, so the device is left alone for them. Some roam tuning fields (the busy ones) can be missing on a driver while the rest of the group is supported; they are then left out of a pin.

## Editing

1. Press **Edit** on the settings panel.
2. For each group, keep **Default (value)** or switch to a pinned value. The default's current value is shown in the label; when there is no default it reads **Default (not managed)**.
3. Save. Only the groups you touched are changed; the pins of the others stay. **Reset all to default** removes every pin.

Saving stores the new wanted values and raises the device's settings version. A banner then reads **Edits not applied yet**, with the version stored and the version the device last applied. The device is not contacted.

You need `devices:write` to edit settings; applying also needs `commands:basic`.

## Apply and Re-apply

**Apply** sends the resolved settings to the device as one command, at the current settings version. After an edit the button reads **Apply**; when the device already runs the current version it reads **Re-apply**, which queues the same version again (useful to correct drift).

Applying driver settings can reload the Wi-Fi driver, so the device goes offline for about 10 to 20 seconds. The panel asks for confirmation first.

Independently of **Apply**, smithd re-checks the settings every 60 seconds and corrects a value that drifted. A value that does not stick is retried, and after two attempts for the same version it is reported as **Failed**.

### A setting left not managed

When a setting is not managed (no pin and no default), smithd removes the file it had written for it. It does not leave the value as it was. For a driver setting, the driver falls back to its own default at the next driver reload (or reboot), not immediately, because removing the setting does not itself reload the driver.

### New devices

A device at settings version 0, one nobody has edited, gets its distribution defaults applied automatically once, as soon as its distribution has defaults. After that, changes wait for an **Apply**. **Apply** skips devices still at version 0 because they have nothing to apply.

## Self revert

If smithd cannot reach the API for 90 seconds after a driver reload or a NetworkManager re-activation, it puts back the files it replaced and reports the version as **Reverted**. This keeps a bad Wi-Fi setting from stranding a device. Fix the pin and apply again.

## States

Each setting shows one of these states, taken from the device's last report.

| State | Meaning |
| --- | --- |
| **Pending** | The device has not applied the current version yet, or has never reported. |
| **In sync** | The device runs the wanted value. |
| **Not matching** | The device applied the version but runs something else now. |
| **Failed** | The device could not set the value; the reason is shown. |
| **Reverted** | The change was reverted by the self revert. |
| **Couldn't read** | The device could not read the current value. |
| **Not used on this device** | The distribution does not support the setting. |
| **Not managed** | Shown as the setting's badge when it has no pin and no default. |

## Bulk edit

On the **Devices** page, select devices to use **Edit settings** and **Apply settings** next to **Deploy to Selected**.

- **Edit settings** needs every selected device on the same distribution, since the defaults and supported settings depend on it. With a mix, the dialog shows a message instead of the form. The form is the same as for one device, without **Hostname**, which is always per device. Only the groups you touch are changed.
- **Apply settings** works on any selection, and queues one staggered bundle with a command per device. Devices with nothing to apply (settings version 0) are skipped. Driver settings can reload the Wi-Fi driver on each device, with the same 90 second self revert.
