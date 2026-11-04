---
name: verify-command-delivery
description: "Model-check that an online smithd never gets stuck and that every command the api queues runs and reports back. Use when a change touches the smithd postman poll loop, the commander or its executor, the channels between them, or the api /home handler and get_commands."
---

# Verify command delivery

The TLA+ model `smithd/formal/CommandDelivery.tla` describes how a command goes from the api to smithd and how its result comes back. TLC checks every order of steps of the postman, the commander, the executor, the other actors that queue reports, the user, and the network, within the bounds in the configs.

Properties, each with the harm when it breaks:

- `KeepsPolling`: the postman reaches `/home` again and again. If it breaks, the device is online but no one can send it commands.
- `EveryCommandAnswered`: every command that a user queues runs, and its result gets to the api. If it breaks, a command stays "pending" in the dashboard for ever.
- `NoCommandLost`: every row with `fetched = true` and no result is still somewhere in smithd. If it breaks, smithd forgot a command that the api will not send again.
- Deadlock: TLC runs with `-deadlock` off, because the postman loop never ends. A real deadlock shows as a `KeepsPolling` violation.

## When to run it

Run it when a diff touches one of the covered places below. A change elsewhere in smithd does not need it, unless the change adds an await to the postman loop or to the commander loop.

## Run

```sh
smithd/formal/check.sh            # current code, must pass (about 40 s)
smithd/formal/check.sh findings   # also checks that each known finding still fails
```

Requirements: Java 11 or later, and `tla2tools.jar` at `~/.local/share/tla/tla2tools.jar`, or set `TLA2TOOLS_JAR`. The model was checked with TLC2 2026.10.20 (rev d2e1472). The script exits non-zero when TLC finds a violation. With `findings`, it also exits non-zero when a known finding passes ("UNEXPECTED PASS"). In that case, a fix landed: update the Findings section and the config.

Read each result with the pstack `/statechart` skill. It gives the shortest path to each invariant violation in plain words, and it draws a chart of the states when the check passes. If `/statechart` cannot read the spec, read the TLC trace: it prints each step as `State N: <ActionName ...>` and the variables after the step.

## Coverage record

Last confirmed in sync at commit `e25058a23cbd80cb91ecf724f68ce0a35c13ee7a`. Move this commit forward only after you confirm that the model describes the code at the new commit.

| Model | Code |
|---|---|
| `Tick`, `SendGet`, `TakeResults` | `smithd/src/postman/mod.rs`, `Postman::run`: `keep_alive_interval.tick()`, `feed_watchdog`, `ensure_token`, `commander.get_results()` |
| `PostOk`, `PostFails`, `PostAnswerLost` | `Postman::ping_home`, `NetworkClient::send_compressed_post` (`smithd/src/utils/network.rs`, 10 s client timeout) |
| `Requeue` | `Postman::run` → `CommanderHandle::requeue_results` |
| `Dispatch` | `CommanderHandle::execute_api_batch` |
| `CommanderInbox`, `CommanderSend`, `CommanderResponse`, `Requeued` | `smithd/src/commander/mod.rs`: `Commander::run`, `requeue_results` (keeps only command results, positive ids; reports are dropped) |
| `ExecStart`, `ExecFinish` | `CommandQueueExecutor::run`, `impl Execute for Handles` |
| `OtherReport` | `CommanderHandle::insert_result` from settings, auditor, nm_watcher and the NM-profile task in `Postman::run` |
| `InboxCap` | `mpsc::channel(10)` in `CommanderHandle::spawn` |
| `InternalCap = 0` | `mpsc::unbounded_channel()` for the executor queue and the responses in `CommanderHandle::spawn` |
| `Store`, `Fetched`, `PostOk` order | `api/src/smith/route.rs` `/home` handler: `save_responses`, then `get_commands`; `api/src/home.rs` `get_commands` |
| `Restart` | `smithd/debian/smithd.service` (`Restart=always`, `WatchdogSec=10min`), police reboots, OTA |

One TLA+ action is one step between two awaits in the code. Bounds: 4 commands, 1 report, an inbox of 1, at most 1 failed post. The inbox of 1 stands for the real 10: a smaller channel fills sooner, so TLC sees the same blocking with fewer commands. A bounded channel of capacity k needs at least 2k + 2 commands to fill both directions, so with `InternalCap = 1` keep `Cmds` at 4 or more.

## Assumptions the model makes (not checked by TLC)

- Each postman step returns. Evidence at the sync commit: HTTP requests use a 10 s client timeout, `check_services` probes use a 5 s timeout, and the magic, session and police actors answer at once or after a request that uses the 10 s timeout. Exception: see finding 4.
- Each command handler returns, except the commands in `Hanging`. See finding 3.
- The executor task does not panic.
- "Online" means a finite number of failed posts (`MaxFailures`). A pass says nothing about a device that never reaches the api.
- Not modeled: `MAX_KEPT_RESULTS` (results dropped only after a long outage), the result payloads, `continue_on_error`, stagger and `available_at`, the token registration and 401 refresh path, and shutdown.

## Findings at the sync commit

`check.sh findings` reproduces findings 1 to 3. Each one must fail until the code is fixed.

1. **A lost `/home` answer loses its commands (`AnswerLost.cfg`, `NoCommandLost`).** `get_commands` commits `fetched = true` before the answer is sent. If the answer does not reach smithd (the 10 s timeout fires, or the connection drops), smithd never sees the commands, and nothing in the api sends them again. Path: user queues command 1 → poll → api stores and marks 1 fetched → answer lost → command 1 stays pending for ever.
2. **A smithd restart loses queued commands (`Restart.cfg`, `NoCommandLost`).** The commander results, the executor queue and the running command exist only in memory. A watchdog restart, a reboot, an OTA, or a crash after a poll deletes them, and the api does not send them again.
3. **One command that does not return blocks all later commands (`HangingHandler.cfg`, `EveryCommandAnswered`).** The executor runs commands one at a time and has no overall timeout, while the postman keeps polling, so the watchdog does not fire. Candidates at the sync commit: `StartOTA` (`tar` and `nv_ota_start.sh` through `std::process::Command::output()` with no timeout, `smithd/src/filemanager/systemactions.rs`), `ExtendedNetworkTest` (loops for `duration_minutes`, which smithd does not clamp, `smithd/src/commander/network.rs` `perform_extended_network_test`), `Restart` (`shutdown -r +1` with no timeout, `smithd/src/commander/restart.rs`). A panic in a handler (the `unwrap()` calls on actor replies in the filemanager, downloader, tunnel, updater and magic handles) stops the executor task with the same effect.
4. **Outside the model: the postman can block on `nmcli`.** `SystemInfo::new` calls `get_connection_statuses` (`smithd/src/utils/system.rs`), which runs `nmcli` with the blocking `std::process::Command::output()` and no timeout. The postman calls it after each command result and every 5 minutes. If NetworkManager hangs, the poll loop stops. The watchdog restarts smithd after 10 minutes, but startup calls `SystemInfo::new` again.

## Real bug or modeling error

When the check fails on the current-code config:

1. Read the trace, and replay it step by step in the code with the coverage table. Each action must be possible at that point in the code.
2. If a step is not possible in the code, the model is wrong. Fix the model, then confirm that each property still fails on a broken variant (next section).
3. If each step is possible, it is a bug. Report it with the trace in the code's words. Do not weaken a property or remove an action to make the check pass.

## When a change is intended

Update the model in the same change:

- A new await in a loop, a new channel, or a changed capacity: add or change the action or constant, and add a row to the coverage table.
- A fix for a finding: change its config so that it shows the fixed behavior, move it into `CommandDelivery.cfg` if it now passes, and remove the finding from this file and from the `findings` loop in `check.sh`.
- Then confirm that the check can still fail. Each of these broken variants must fail on `CommandDelivery.cfg`:
  - `InternalCap = 1`: the bounded executor channels before commit e56022f. Expect `KeepsPolling` and `EveryCommandAnswered` to fail.
  - `Requeue` without the requeue message (as if `Postman::run` did not call `requeue_results`). Expect `NoCommandLost` to fail.

Last, move the sync commit forward.
