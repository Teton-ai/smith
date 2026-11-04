---
name: verify-restart-channel
description: "Model-check that a Restart command reboots a device through smith-rescue exactly once, also when answers are lost or processes restart. Use when a change touches api/src/restart.rs, the /smith/restart route, the Restart exclusion in get_commands, or rescue/src."
---

# Verify the restart channel

The TLA+ model `rescue/formal/RestartChannel.tla` describes how a Restart command goes from the api to smith-rescue, and how the api learns that the device rebooted. TLC checks every order of steps of the user, smith-rescue, the api, and the environment: lost answers, smith-rescue restarts, other reboots (power cut, police, OTA), and reboot procedures that fail.

Properties, each with the harm when it breaks:

- `DoneMeansRebooted`: a Restart is "done" only when the boot that it had to end did end, and that boot was not already over when the user queued it. If it breaks, the dashboard says a device rebooted when it did not.
- `RebootsOnlyForThisBoot`: smith-rescue reboots only for a Restart that waits for the current boot. If it breaks, a Restart can reboot a device twice.
- `EveryRestartEnds`: every queued Restart ends with a reboot. If it breaks, a Restart can stay pending for ever.

smithd is not in the model, because smith-rescue does not depend on it. The e2e test `e2e/tests/restart.rs` shows this on a real device container: it freezes smithd with SIGSTOP and still gets a reboot.

## When to run it

Run it when a diff touches a covered place below. The e2e test runs with the e2e suite (`make test.e2e`).

## Run

```sh
rescue/formal/check.sh          # the design in the code, must pass (a few seconds)
rescue/formal/check.sh broken   # also checks that two broken designs fail
```

Requirements: Java 11 or later, and `tla2tools.jar` at `~/.local/share/tla/tla2tools.jar`, or set `TLA2TOOLS_JAR`. Checked with TLC2 2026.10.20 (rev d2e1472). The script exits non-zero when TLC finds a violation, and with `broken` also when a broken design passes ("UNEXPECTED PASS").

Read each result with the pstack `/statechart` skill. It gives the shortest path to each invariant violation in plain words, and it draws a chart when the check passes. If `/statechart` cannot read the spec, read the TLC trace: each step prints as `State N: <ActionName ...>` with the variables after it.

## Coverage record

Last confirmed in sync with the commit that adds this skill. Record a commit here only after you confirm that the model describes the code at that commit.

| Model | Code |
|---|---|
| `UserQueues` | a `"Restart"` row in `command_queue` (dashboard, `sm restart device`, MCP `restart_devices`) |
| `SendPoll` | `rescue/src/main.rs` `must_reboot`: reads `/proc/sys/kernel/random/boot_id`, `GET {server}/restart?boot_id=` |
| `TakeAnswer` | `must_reboot` → `is_this_boot` |
| `Reboot`, `RebootFails` | `rescue/src/reboot.rs` `reboot`: v2 power-board reset through `esp-host`, `systemctl reboot`, `--force --force`, `reboot(2)` |
| `ApiPoll`, `Completes` | `api/src/restart.rs` `poll`: one transaction under the advisory lock `(3, device_id)`: record the poll, complete deliveries of another boot, hand out waiting ones bound to this boot |
| `AnswerLost` | the 10 s client timeout in `rescue/src/main.rs`, or a dropped connection |
| `RescueCrash` | systemd restarts `smith-rescue.service` (`Restart=always`, `WatchdogSec=5min`) |
| `OtherReboot` | any reboot that smith-rescue did not do |

Not in the model, and checked by the api tests in `api/src/restart.rs` instead: `get_commands` in `api/src/home.rs` leaves Restarts to a channel that polled within `LIVE_CHANNEL` (3 minutes) and gives them to smithd after that; a staggered Restart waits for its `available_at`.

Bounds: 2 Restarts, at most 1 of each environment event. Two Restarts are enough to show that two clicks give one reboot.

## Assumptions (not checked by TLC)

- A poll reaches the api before a boot ends. The client gives up after 10 s, the api's statements after 3 s, and a boot takes longer. Without this, a poll sent just before a power cut could bind a Restart queued after the cut to the old boot, and the api would mark it done without a reboot.
- Each step of smith-rescue's loop ends: one thread, a timeout on each HTTP call and each child process, and `wait` feeds the systemd watchdog.
- The device check in `is_this_boot` is defense in depth: in the model an answer cannot outlive the process that polled, so the api's binding alone keeps the properties.

## Real bug or modeling error

When `check.sh` fails on `RestartChannel.cfg`, replay the trace step by step in the code with the coverage table. If a step cannot happen in the code, fix the model and confirm that the broken designs still fail. If each step can happen, it is a bug: report it with the trace in the code's words. Do not weaken a property to make the check pass.

## When a change is intended

Change the model in the same diff, and update the coverage table. Then confirm that the check can still fail:

- `DoneOnDelivery.cfg`: the api marks a Restart done when it hands it out. `DoneMeansRebooted` must fail.
- `CompleteOnNextPoll.cfg`: the api marks it done on the next poll from any boot, as an ack would. `DoneMeansRebooted` must fail.

Last, move the sync commit forward.
