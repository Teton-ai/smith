---
title: WiFi management
description: See what a device is connected to, scan for nearby networks, and push a prioritised list of networks to it safely.
---

The **WiFi** panel on a device's **Network** tab shows three things, from most passive to most powerful:

| Part | Question it answers | Changes the device? |
| --- | --- | --- |
| **Profiles** | What networks does the device know about right now? | No |
| **Scan** | What networks can the device hear right now? | No |
| **Intent** | What networks should the device know about? | Yes, when you press **Apply** |

Profiles and scan describe what is true on the device. Intent is what you want to be true. Nothing you do to the intent list reaches the device until you apply it.

## Profiles

A profile is a saved NetworkManager WiFi connection on the device. The **Configured profiles** list shows, for each one:

- The profile name (the NetworkManager connection name) and the SSID beneath it.
- A security badge and a **Hidden** badge when the SSID is not broadcast.
- The credentials, with the password masked until you reveal it. Enterprise networks also show their identity.
- An **Active** badge on the profile the device is connected to right now.

The panel also shows the current network and a **Last checked** time.

### How the list stays fresh

The device reports its profiles on its own when smithd starts and whenever NetworkManager sees a connectivity change, a profile added, or a profile removed (changes are debounced by about two seconds). The dashboard only shows what the device has already reported, and it is not pushed anything.

While the device's **Network** tab is open, the panel re-reads the stored profiles (and the intent list) every 30 seconds, and also when you return to the browser window. That polling pauses while the browser tab is in the background and stops when you leave the tab, so nothing is refreshed for a device whose page is not open. Opening the tab always loads the latest stored data, and a change made on the device appears within about 30 seconds once the device has reported it.

**Refresh** asks the device to report immediately. It shows **Syncing...** until the device answers, and gives up after 45 seconds, which is what you will see for an offline device.

### Two profiles with the same SSID

Profiles are keyed by name, not SSID. Two profiles pointing at the same network appear as two rows.

### Passwords

Anyone with `devices:read` can reveal a device's WiFi passwords in the dashboard. The stored command history redacts them, but the panel does not.

### Add to intent

A configured profile that is not in the intent list has an **Add to intent** button, which appends it at the bottom of the intent. Profiles already covered show **In intent**. Adding it does not touch the device; see [Intent](#intent).

## Scan

**Scan WiFi** asks the device to list the access points it can currently see. Results replace the device's previous scan, but only when a scan succeeds, so a failed scan never wipes the last good result. Before the first scan the panel says **Never scanned**.

### Reading the results

A device typically sees dozens of access points for a handful of networks, so results are grouped by SSID and security type. Each group shows its best signal and expands to the individual access points (BSSID, signal, band and channel, rate). Groups sort by best signal.

Each group can carry a relationship badge, matched on SSID and security type against the device's profiles and intent:

- **Active**: the device is connected to it.
- **Configured**: the device has a profile for it.
- **In intent**: it is in the intent list.

The filters (text, band, security) narrow which access points show when you expand a group.

### Hidden networks

Access points that do not broadcast their SSID cannot be named by a normal scan and collect in a single **hidden APs** group, always listed last. To resolve the ones the device is configured for, smithd also probes the SSIDs of its own hidden profiles (at most five, highest autoconnect priority first) and merges what it finds. A hidden network the device has no profile for stays anonymous.

### Scanning many devices

The devices page can send **WifiScan** to a selection of devices. Bulk scans are dispatched in staggered waves (two devices every ten seconds), because devices scanning at the same moment interfere with each other and skew the results. A device further back in the queue therefore starts its scan later than the first ones.

## Intent

The intent is an ordered list of networks for one device. Each entry links to a network in the shared network catalog, so a password lives in one place and can be reused across devices.

### Editing the list

- **Add network** picks from the catalog, or creates a catalog entry (name, SSID, optional password, hidden) and adds it in one step. An empty password creates an open network. New entries go to the bottom.
- The arrows reorder entries. **Position is priority**: the top entry is the network the device prefers.
- The bin removes an entry, after a confirmation.
- The same catalog network cannot be added twice. Two different catalog entries with the same SSID can coexist.

Every edit, including moving an entry down and back up, marks the intent as changed. Nothing reaches the device until you press **Apply**, and removing an entry does not delete its profile from the device until the next apply.

### The sync chip

The chip next to the **Intent** heading compares the version of the list with the version the device last confirmed.

| State | Meaning |
| --- | --- |
| **Unknown** | The device has never reported an applied version. |
| **Pending** | The list has changed since the device last confirmed. Either you have not applied yet, or the device has not answered. |
| **Applying...** | You pressed **Apply** and the dashboard is waiting, polling every three seconds for up to 60 seconds. |
| **Synced** | The device applied the current list with no failures. |
| **Error** | The device applied the list but at least one entry failed. Click the chip for the reason per profile. |

A chip that falls back to **Pending** after Apply means no confirmation arrived: the device is offline, has not polled yet, or the apply could not run at all (see [Hard failures](#hard-failures)).

### What Apply does

Apply sends the whole list to the device as a single command, and the device reconciles itself to it. The API and the device both enforce rules meant to keep the device reachable. You need `devices:write` to edit and apply an intent.

#### Before anything is sent

- The list must not be empty, so the **Apply** button is disabled for an empty list. There is no way to apply "no networks": removing the last entry only changes the database, and the device keeps its profiles.
- Only **WPA/WPA2 personal** and **open** networks can be applied. Entries of any other type (WPA3 SAE, Enhanced Open, enterprise, WEP) are dropped from the command by the API. They are shown in the intent list but never configured, and they do not raise an error. If nothing applicable is left, the request is rejected.
- Every entry needs an SSID, and a secured entry needs a password.
- Priorities are recomputed from the list order at apply time (`(entries - position) * 10`), so gaps left by reordering never matter and the raw database values never reach the device.

#### How the device treats each entry

For each entry, in list order, the device looks for a profile with the entry's name:

| Situation | What happens |
| --- | --- |
| No profile with that name, but the same network exists under another name (same SSID, hidden flag, security type and password) | The existing profile is **adopted**: renamed to the intent name instead of duplicated. If several match, the active one wins. |
| No such profile at all | A new profile is created with the entry's priority. |
| A profile with that name exists and is **not active** | It is overwritten in place with the entry's SSID, password, hidden flag and priority. |
| A profile with that name exists and is **active**, and nothing changed | Only its priority is updated. The connection is not restarted. |
| A profile with that name exists and is **active**, and the SSID or password changed | The [connectivity guard](#the-connectivity-guard) runs. |

**Note:** An inactive profile is edited without connecting to it, so a wrong password on a network the device is not currently using is not caught at apply time. It surfaces later, when the device tries to use that network. Only changes to the active network are tested first.

#### The connectivity guard

Changing the password or SSID on the network the device is using right now is the most dangerous edit, because a mistake strands the device offline. For a secured active profile, the device does not edit it in place:

1. It creates a temporary profile with the new credentials and autoconnect off.
2. It tries to connect using that temporary profile.
3. If the connection succeeds, it deletes the old profile, renames the temporary one to the intended name, and turns autoconnect on.
4. If the connection fails, it reconnects the original profile, deletes the temporary one, and reports the failure (`WrongPSK`, `NotInRange` or `NmcliError`). The device ends up exactly where it started.

An active open network has no password to validate, so a changed one is edited in place without this test.

#### What gets deleted

The device keeps a record of the profiles it created or adopted through Apply (`/etc/smith/last-applied-networks.json`). Cleanup is based on that record, not on a comparison with the intent list:

- A profile that Smith manages and that is no longer in the intent is deleted.
- A profile that Smith never touched is never deleted, even if it is not in the intent. Profiles that the device already had are only affected if an intent entry has the same name or matches one exactly (see the table above), and from then on they are managed.
- Inactive profiles are deleted first. The active profile is deleted last, and only after the device has managed to connect to another network, trying the intent entries in priority order and then any other profile it has.
- If no fallback connects, the active profile is **kept** and reported as `ActiveProfileKept`. It stays on the record and is retried on the next apply.
- A profile whose deletion fails stays on the record. A partial failure never causes the device to lose track of profiles that still exist.

#### Priority does not force a switch

Autoconnect priority only decides which network the device picks when it starts a new connection. Lowering the priority of the network the device is on, or raising another one, does not disconnect it. The device stays on its current network until that connection ends by itself.

### Errors

An apply that reached the device and partly failed still counts as applied: the version is confirmed and the chip shows **Error**. Expand it to see one line per failed profile:

| Reason | Meaning |
| --- | --- |
| `WrongPSK` | The connectivity guard could not authenticate with the new password. The old profile was restored. |
| `NotInRange` | The connectivity guard could not find the network. The old profile was restored. |
| `NmcliError` | A NetworkManager command failed, or the entry was not applicable (unsupported security type, or a secured entry with no password). |
| `ActiveProfileKept` | A removed profile is the active connection and no fallback network was reachable, so it was not deleted. |

#### Hard failures

If the device cannot list its profiles, or cannot read or write its record of managed profiles, it aborts and does not confirm the version. The chip stays **Pending**. Check the device's logs (`smithd`) for the cause.

### A safe way to change credentials

1. Add the new network and leave the old one in place, below it.
2. Apply, and wait for **Synced**.
3. Confirm from **Scan** and **Profiles** that the device sees and has connected to the new network.
4. Remove the old entry and apply again.

If step 2 fails, the old network is still there to fall back on.

### Limits worth knowing

- A pre-existing profile that shares a name with an intent entry is overwritten by that entry.
- WPA3, Enhanced Open and enterprise networks can be added to the intent but are silently skipped by Apply.
- Intent is per device. There is no fleet-wide apply.
