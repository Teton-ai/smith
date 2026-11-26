---
title: Device facts
description: Let any package on a device report small read-only values, such as a public key or a hardware variant, by dropping a file into /etc/smith/facts.d.
---

A **fact** is a small read-only value a device reports about itself. Any package on the device can publish one by putting a file in `/etc/smith/facts.d`. smithd reads the folder and sends the values to Smith with the rest of the device's system info.

Facts fill the gap between the other two kinds of device data:

| | Set by | Direction |
| --- | --- | --- |
| **Labels** | You, in Smith | Stay in Smith |
| **Variables** | You, in Smith | Sent down to the device |
| **Facts** | Software on the device | Reported up to Smith |

Use a fact for something only the device knows: a public key it generated, a hardware variant, the mode a component runs in. Never put a secret in a fact. Anyone who can read devices in Smith can read every fact.

## Reporting a fact

Write a file into `/etc/smith/facts.d`. The file name is the fact's name and the file's content is its value:

```bash
echo "rev-c" > /etc/smith/facts.d/acme-board-revision
```

The device then reports:

```json
"facts": {
  "acme-board-revision": "rev-c"
}
```

A symlink works too. smithd follows it and reports the file it points to, which is the easiest way to publish a value your own daemon already keeps up to date:

```bash
ln -s /var/lib/acme/device-key.pub /etc/smith/facts.d/acme-device-key
```

Prefix fact names with the name of your package, so two packages never pick the same one.

## Rules

smithd skips a file, and logs why, when it does not fit these rules. A skipped file never affects the other facts.

| Rule | Why |
| --- | --- |
| The name does not start with `.` | Dotfiles are for temporary files, see [Writing a fact safely](#writing-a-fact-safely). |
| It is a regular file, after following symlinks | Folders, devices and broken symlinks are skipped. |
| It is world-readable (`o+r`) | Guards against reporting a private file by mistake, for example a symlink to a private key instead of its public half. |
| It is at most 4 KiB | Facts are values, not documents. |
| The name and content are UTF-8 | Facts are reported as JSON strings. Encode binary data yourself, for example as base64. |

Whitespace around the value is trimmed, so a trailing newline is fine. A device reports at most 32 facts. When there are more files than that, which ones are kept is not defined.

## When facts are reported

smithd reads the folder whenever it collects system info:

- when smithd starts,
- every 5 minutes, and
- right after the device finishes a command.

The system info is only sent when something in it changed, so a fact that never changes costs nothing after the first report. A new or changed fact reaches Smith within 5 minutes, and a fact whose file is removed disappears from the report the same way.

## Writing a fact safely

smithd can read the folder at any moment, including while your package is writing a file. Two habits keep it from reporting something half-written:

- **Write to a dotfile, then rename it.** Write the value to `.acme-board-revision.tmp`, then `mv` it to `acme-board-revision`. smithd ignores dotfiles, and a rename is atomic, so it only ever sees the old value or the new one.
- **Create a file with its final permissions.** Writing a file and changing its mode afterwards leaves a moment where it has the default mode. For a public fact that only matters if the default is not world-readable, but for a private file next to it, such as a private key, create it with its restrictive mode from the start.

A symlink to a file your daemon already writes atomically needs neither.

## Reading facts

Facts are part of a device's system info, under `system_info.facts`. Any request that returns devices returns them, for example every device with a label:

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "$SMITH_API_URL/devices?labels=site=north&limit=1000"
```

```json
[
  {
    "serial_number": "DEV-0042",
    "system_info": {
      "facts": {
        "acme-device-key": "k3Jf...="
      }
    }
  }
]
```

A device with no facts has no `facts` field. `GET /devices` returns 100 devices by default and at most 1000 per page; read the `x-total-count` header to know whether you got them all. See the [API reference](/docs/api) for every filter.
