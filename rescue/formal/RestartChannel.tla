---------------------------- MODULE RestartChannel ----------------------------
(***************************************************************************)
(* The restart channel: a Restart command from the api to smith-rescue,   *)
(* and the proof of the reboot back.                                       *)
(*                                                                         *)
(*   api     api/src/restart.rs (poll), api/src/smith/route.rs           *)
(*           (restart_poll)                                                *)
(*   rescue  rescue/src/main.rs (main loop, must_reboot),                 *)
(*           rescue/src/reboot.rs (reboot)                                 *)
(*                                                                         *)
(* Boots are numbered; a real boot id is a new random UUID at each boot.   *)
(* smithd is not in the model: smith-rescue does not depend on it.        *)
(***************************************************************************)
EXTENDS Integers

CONSTANTS
    Restarts,      \* Restart command ids
    MaxOtherBoots, \* reboots not caused by smith-rescue (power cut, police)
    MaxLost,       \* poll answers lost on the way back
    MaxCrashes,    \* smith-rescue restarts by systemd
    MaxFailed,     \* reboot procedures that leave the device up
    Variant        \* "real", or a broken design: "doneOnDelivery", "completeOnNextPoll"

VARIABLES
    cmd,        \* state of each command_queue row: new, queued, delivered, done
    target,     \* restart_delivery.target_boot, 0 when none
    queuedIn,   \* the boot in which the user queued the Restart (history)
    boot,       \* the device's current boot
    pc,         \* smith-rescue: idle, polling, rebooting
    request,    \* boot id of the poll in flight, 0 when none
    answer,     \* target_boot of the answer in flight: -1 none, 0 null, else a boot
    rescueReboots, \* boots ended by smith-rescue
    otherBoots, lost, crashes, failed

vars == <<cmd, target, queuedIn, boot, pc, request, answer, rescueReboots,
          otherBoots, lost, crashes, failed>>
env == <<otherBoots, lost, crashes, failed>>

NoAnswer == -1

Init ==
    /\ cmd = [c \in Restarts |-> "new"]
    /\ target = [c \in Restarts |-> 0]
    /\ queuedIn = [c \in Restarts |-> 0]
    /\ boot = 1
    /\ pc = "idle"
    /\ request = 0
    /\ answer = NoAnswer
    /\ rescueReboots = {}
    /\ otherBoots = 0 /\ lost = 0 /\ crashes = 0 /\ failed = 0

---------------------------------------------------------------------------
UserQueues(c) ==
    /\ cmd[c] = "new"
    /\ cmd' = [cmd EXCEPT ![c] = "queued"]
    /\ queuedIn' = [queuedIn EXCEPT ![c] = boot]
    /\ UNCHANGED <<target, boot, pc, request, answer, rescueReboots, env>>

---------------------------------------------------------------------------
(* smith-rescue: main loop and must_reboot.                               *)

\* Reads the boot id and sends GET /smith/restart?boot_id=.
SendPoll ==
    /\ pc = "idle"
    /\ request = 0
    /\ answer = NoAnswer
    /\ pc' = "polling"
    /\ request' = boot
    /\ UNCHANGED <<cmd, target, queuedIn, boot, answer, rescueReboots, env>>

\* is_this_boot: reboot only for this boot. An answer cannot outlive the
\* process that polled, so this check is defense in depth.
TakeAnswer ==
    /\ pc = "polling"
    /\ answer # NoAnswer
    /\ pc' = IF answer = boot THEN "rebooting" ELSE "idle"
    /\ answer' = NoAnswer
    /\ UNCHANGED <<cmd, target, queuedIn, boot, request, rescueReboots, env>>

\* reboot::reboot takes the device down.
Reboot ==
    /\ pc = "rebooting"
    /\ rescueReboots' = rescueReboots \cup {boot}
    /\ boot' = boot + 1
    /\ pc' = "idle"
    /\ answer' = NoAnswer
    /\ UNCHANGED <<cmd, target, queuedIn, request, env>>

\* Every step of reboot::reboot failed; the next poll tries again.
RebootFails ==
    /\ pc = "rebooting"
    /\ failed < MaxFailed
    /\ failed' = failed + 1
    /\ pc' = "idle"
    /\ UNCHANGED <<cmd, target, queuedIn, boot, request, answer, rescueReboots,
                   otherBoots, lost, crashes>>

Rescue == SendPoll \/ TakeAnswer \/ Reboot \/ RebootFails

---------------------------------------------------------------------------
(* api: restart::poll, one transaction under the device's advisory lock.  *)

\* "completeOnNextPoll" is the ack design: any later poll completes it.
Completes(c, b) ==
    CASE Variant = "doneOnDelivery" -> FALSE
      [] Variant = "completeOnNextPoll" -> cmd[c] = "delivered"
      [] OTHER -> cmd[c] = "delivered" /\ target[c] # b

ApiPoll ==
    /\ request # 0
    /\ answer = NoAnswer
    /\ LET b == request IN
       /\ LET after == [c \in Restarts |->
                    IF Completes(c, b) THEN "done"
                    ELSE IF cmd[c] = "queued"
                         THEN IF Variant = "doneOnDelivery" THEN "done" ELSE "delivered"
                    ELSE cmd[c]]
                handed == {c \in Restarts : cmd[c] = "queued"}
            IN
            /\ cmd' = after
            /\ target' = [c \in Restarts |-> IF c \in handed THEN b ELSE target[c]]
            \* A poll from a process that is gone gets an answer that nobody reads.
            /\ answer' = IF pc # "polling" THEN NoAnswer
                         ELSE IF \E c \in Restarts :
                              (after[c] = "delivered" /\ target'[c] = b)
                              \/ (Variant = "doneOnDelivery" /\ c \in handed)
                         THEN b ELSE 0
    /\ request' = 0
    /\ UNCHANGED <<queuedIn, boot, pc, rescueReboots, env>>

---------------------------------------------------------------------------
(* Environment                                                            *)

\* The 10 s timeout fires or the connection drops on the way back.
AnswerLost ==
    /\ answer # NoAnswer
    /\ lost < MaxLost
    /\ lost' = lost + 1
    /\ answer' = NoAnswer
    /\ pc' = "idle"
    /\ UNCHANGED <<cmd, target, queuedIn, boot, request, rescueReboots,
                   otherBoots, crashes, failed>>

\* systemd restarts smith-rescue; the device stays up. A poll in flight
\* still reaches the api.
RescueCrash ==
    /\ crashes < MaxCrashes
    /\ crashes' = crashes + 1
    /\ pc' = "idle"
    /\ answer' = NoAnswer
    /\ UNCHANGED <<cmd, target, queuedIn, boot, request, rescueReboots,
                   otherBoots, lost, failed>>

\* A power cut, the police, or an OTA reboots the device. Assumption: a poll
\* in flight reaches the api before a boot ends, since the client gives up
\* after 10 s, the api's statements after 3 s, and a boot takes longer.
OtherReboot ==
    /\ request = 0
    /\ otherBoots < MaxOtherBoots
    /\ otherBoots' = otherBoots + 1
    /\ boot' = boot + 1
    /\ pc' = "idle"
    /\ answer' = NoAnswer
    /\ UNCHANGED <<cmd, target, queuedIn, request, rescueReboots, lost, crashes, failed>>

Next ==
    \/ \E c \in Restarts : UserQueues(c)
    \/ Rescue
    \/ ApiPoll
    \/ AnswerLost \/ RescueCrash \/ OtherReboot

Spec ==
    /\ Init /\ [][Next]_vars
    /\ WF_vars(Rescue)
    /\ WF_vars(ApiPoll)

---------------------------------------------------------------------------
(* Properties                                                             *)

TypeOK ==
    /\ cmd \in [Restarts -> {"new", "queued", "delivered", "done"}]
    /\ pc \in {"idle", "polling", "rebooting"}

\* "done" is proof: the boot the Restart had to end did end, and that boot
\* was not over before the user queued the Restart.
DoneMeansRebooted ==
    \A c \in Restarts : cmd[c] = "done" =>
        /\ target[c] # 0
        /\ target[c] < boot
        /\ target[c] >= queuedIn[c]

\* smith-rescue reboots only for a Restart that waits for this boot, so no
\* Restart reboots twice.
RebootsOnlyForThisBoot ==
    [][Reboot => \E c \in Restarts : cmd[c] = "delivered" /\ target[c] = boot]_vars

\* Every queued Restart ends with a reboot.
EveryRestartEnds ==
    \A c \in Restarts : cmd[c] = "queued" ~> cmd[c] = "done"
=============================================================================
