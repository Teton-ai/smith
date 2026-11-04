---------------------------- MODULE CommandDelivery ----------------------------
(***************************************************************************)
(* How a command travels from the api to smithd and its result travels    *)
(* back, while the device is online.                                       *)
(*                                                                         *)
(*   api        api/src/smith/route.rs (/home), api/src/home.rs            *)
(*              (save_responses, get_commands)                             *)
(*   postman    smithd/src/postman/mod.rs  (Postman::run loop)             *)
(*   commander  smithd/src/commander/mod.rs (Commander::run)               *)
(*   executor   smithd/src/commander/mod.rs (CommandQueueExecutor::run)    *)
(*   others     settings, auditor, nm_watcher and the NM-profile task,     *)
(*              which queue reports on the commander inbox                 *)
(*                                                                         *)
(* One action is one step between two awaits in the code.                  *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Cmds,          \* command ids the api can queue (positive in the code)
    Reports,       \* synthetic report ids other actors queue (negative in the code)
    InboxCap,      \* capacity of the commander inbox: mpsc::channel(10)
    InternalCap,   \* capacity of the channels between commander and executor; 0 = unbounded
    MaxFailures,   \* failed /home posts in a run: online means finitely many
    LoseResponse,  \* a /home answer can be lost after the api committed it
    MaxRestarts,   \* smithd restarts (watchdog, OTA, crash) in a run
    Hanging        \* commands whose handler never returns

VARIABLES
    api,          \* state of each command_queue row: new, queued, fetched, done
    pc,           \* postman program counter
    drained,      \* results the postman took with get_results
    batch,        \* commands of the last /home answer not yet sent to the commander
    inbox,        \* commander inbox (bounded)
    reply,        \* get_results oneshot: empty until the commander sends the results
    queue,        \* from the commander to the executor
    running,      \* command the executor runs, or 0
    responses,    \* from the executor to the commander
    results,      \* commander results map: for each id: none, queued or completed
    pending,      \* command the commander waits to send to a full executor queue, or 0
    reportsLeft,  \* reports other actors will still queue
    failures,
    restarts

vars == <<api, pc, drained, batch, inbox, reply, queue, running, responses,
          results, pending, reportsLeft, failures, restarts>>

Ids == Cmds \cup Reports
InboxMsgs ==
    [type : {"queue"}, id : Cmds] \cup [type : {"get"}] \cup
    [type : {"requeue"}, ids : SUBSET Ids] \cup [type : {"report"}, id : Reports]

Room(ch, cap) == cap = 0 \/ Len(ch) < cap
SeqSet(s) == {s[i] : i \in 1..Len(s)}

TypeOK ==
    /\ api \in [Cmds -> {"new", "queued", "fetched", "done"}]
    /\ pc \in {"tick", "get", "await", "post", "requeue", "dispatch"}
    /\ drained \subseteq Ids
    /\ inbox \in Seq(InboxMsgs) /\ Len(inbox) <= InboxCap
    /\ reply \in Seq(SUBSET Ids) /\ Len(reply) <= 1
    /\ running \in Cmds \cup {0}
    /\ results \in [Ids -> {"none", "queued", "completed"}]

Init ==
    /\ api = [c \in Cmds |-> "new"]
    /\ pc = "tick"
    /\ drained = {}
    /\ batch = <<>>
    /\ inbox = <<>>
    /\ reply = <<>>
    /\ queue = <<>>
    /\ running = 0
    /\ responses = <<>>
    /\ results = [i \in Ids |-> "none"]
    /\ pending = 0
    /\ reportsLeft = Reports
    /\ failures = 0
    /\ restarts = 0

---------------------------------------------------------------------------
(* User: queues a command in the dashboard or with `sm`.                  *)
UserQueues(c) ==
    /\ api[c] = "new"
    /\ api' = [api EXCEPT ![c] = "queued"]
    /\ UNCHANGED <<pc, drained, batch, inbox, reply, queue, running, responses,
                   results, pending, reportsLeft, failures, restarts>>

---------------------------------------------------------------------------
(* Postman: one keep_alive_interval tick of Postman::run.                 *)

\* feed_watchdog, ensure_token (device has a token).
Tick ==
    /\ pc = "tick"
    /\ pc' = "get"
    /\ UNCHANGED <<api, drained, batch, inbox, reply, queue, running, responses,
                   results, pending, reportsLeft, failures, restarts>>

\* CommanderHandle::get_results: send GetResults (waits while the inbox is full).
SendGet ==
    /\ pc = "get"
    /\ Room(inbox, InboxCap)
    /\ inbox' = Append(inbox, [type |-> "get"])
    /\ pc' = "await"
    /\ UNCHANGED <<api, drained, batch, reply, queue, running, responses,
                   results, pending, reportsLeft, failures, restarts>>

\* get_results: `rx.await`.
TakeResults ==
    /\ pc = "await"
    /\ reply # <<>>
    /\ drained' = reply[1]
    /\ reply' = <<>>
    /\ pc' = "post"
    /\ UNCHANGED <<api, batch, inbox, queue, running, responses, results,
                   pending, reportsLeft, failures, restarts>>

Fetch == {c \in Cmds : api[c] = "queued"}
Store(a) == [c \in Cmds |-> IF c \in drained /\ a[c] = "fetched" THEN "done" ELSE a[c]]
Fetched(a) == [c \in Cmds |-> IF c \in Fetch THEN "fetched" ELSE a[c]]
AsSeq(S) == CHOOSE s \in [1..Cardinality(S) -> S] : SeqSet(s) = S

\* ping_home 200: save_responses, then get_commands marks the rows fetched.
PostOk ==
    /\ pc = "post"
    /\ api' = Fetched(Store(api))
    /\ batch' = AsSeq(Fetch)
    /\ drained' = {}
    /\ pc' = "dispatch"
    /\ UNCHANGED <<inbox, reply, queue, running, responses, results,
                   pending, reportsLeft, failures, restarts>>

\* ping_home None before the api stored anything (timeout, refused, 5xx).
PostFails ==
    /\ pc = "post"
    /\ failures < MaxFailures
    /\ failures' = failures + 1
    /\ pc' = "requeue"
    /\ UNCHANGED <<api, drained, batch, inbox, reply, queue, running, responses,
                   results, pending, reportsLeft, restarts>>

\* The api committed results and fetched rows, but the answer never reached
\* smithd: the 10 s reqwest timeout fired, or the connection dropped.
PostAnswerLost ==
    /\ LoseResponse
    /\ pc = "post"
    /\ failures < MaxFailures
    /\ failures' = failures + 1
    /\ api' = Fetched(Store(api))
    /\ pc' = "requeue"
    /\ UNCHANGED <<drained, batch, inbox, reply, queue, running, responses,
                   results, pending, reportsLeft, restarts>>

\* CommanderHandle::requeue_results (skips an empty list).
Requeue ==
    /\ pc = "requeue"
    /\ \/ /\ drained = {}
          /\ UNCHANGED inbox
       \/ /\ drained # {}
          /\ Room(inbox, InboxCap)
          /\ inbox' = Append(inbox, [type |-> "requeue", ids |-> drained])
    /\ drained' = {}
    /\ pc' = "tick"
    /\ UNCHANGED <<api, batch, reply, queue, running, responses, results,
                   pending, reportsLeft, failures, restarts>>

\* CommanderHandle::execute_api_batch: one QueueCommand per command.
Dispatch ==
    /\ pc = "dispatch"
    /\ \/ /\ batch = <<>>
          /\ pc' = "tick"
          /\ UNCHANGED <<batch, inbox>>
       \/ /\ batch # <<>>
          /\ Room(inbox, InboxCap)
          /\ inbox' = Append(inbox, [type |-> "queue", id |-> Head(batch)])
          /\ batch' = Tail(batch)
          /\ UNCHANGED pc
    /\ UNCHANGED <<api, drained, reply, queue, running, responses, results,
                   pending, reportsLeft, failures, restarts>>

Postman == Tick \/ SendGet \/ TakeResults \/ PostOk \/ PostFails \/ PostAnswerLost
           \/ Requeue \/ Dispatch

---------------------------------------------------------------------------
(* Commander::run: one select branch. With unbounded executor channels  *)
(* (InternalCap = 0) it awaits nothing inside the loop. With bounded ones, *)
(* `queue.send().await` holds the branch until the executor queue has room. *)

\* Only command results come back; reports are dropped (#710).
Requeued(ids) ==
    [i \in Ids |-> IF i \in ids /\ i \in Cmds /\ results[i] = "none" THEN "completed" ELSE results[i]]

CommanderInbox ==
    /\ pending = 0
    /\ inbox # <<>>
    /\ LET m == Head(inbox) IN
         \/ /\ m.type = "queue"
            /\ results' = [results EXCEPT ![m.id] = "queued"]
            /\ IF Room(queue, InternalCap)
                 THEN /\ queue' = Append(queue, m.id)
                      /\ UNCHANGED pending
                 ELSE /\ pending' = m.id
                      /\ UNCHANGED queue
            /\ UNCHANGED reply
         \/ /\ m.type = "get"
            /\ reply' = <<{i \in Ids : results[i] = "completed"}>>
            /\ results' = [i \in Ids |-> IF results[i] = "completed" THEN "none" ELSE results[i]]
            /\ UNCHANGED <<queue, pending>>
         \/ /\ m.type = "requeue"
            /\ results' = Requeued(m.ids)
            /\ UNCHANGED <<queue, reply, pending>>
         \/ /\ m.type = "report"
            /\ results' = [results EXCEPT ![m.id] = "completed"]
            /\ UNCHANGED <<queue, reply, pending>>
    /\ inbox' = Tail(inbox)
    /\ UNCHANGED <<api, pc, drained, batch, running, responses, reportsLeft,
                   failures, restarts>>

CommanderSend ==
    /\ pending # 0
    /\ Room(queue, InternalCap)
    /\ queue' = Append(queue, pending)
    /\ pending' = 0
    /\ UNCHANGED <<api, pc, drained, batch, inbox, reply, running, responses,
                   results, reportsLeft, failures, restarts>>

CommanderResponse ==
    /\ pending = 0
    /\ responses # <<>>
    /\ results' = [results EXCEPT ![Head(responses)] = "completed"]
    /\ responses' = Tail(responses)
    /\ UNCHANGED <<api, pc, drained, batch, inbox, reply, queue, running,
                   pending, reportsLeft, failures, restarts>>

Commander == CommanderInbox \/ CommanderSend \/ CommanderResponse

---------------------------------------------------------------------------
(* CommandQueueExecutor::run: one command at a time.                       *)

ExecStart ==
    /\ running = 0
    /\ queue # <<>>
    /\ running' = Head(queue)
    /\ queue' = Tail(queue)
    /\ UNCHANGED <<api, pc, drained, batch, inbox, reply, responses, results,
                   pending, reportsLeft, failures, restarts>>

\* `Execute::execute` returns, then `responses.send`.
ExecFinish ==
    /\ running # 0
    /\ running \notin Hanging
    /\ Room(responses, InternalCap)
    /\ responses' = Append(responses, running)
    /\ running' = 0
    /\ UNCHANGED <<api, pc, drained, batch, inbox, reply, queue, results,
                   pending, reportsLeft, failures, restarts>>

Executor == ExecStart \/ ExecFinish

---------------------------------------------------------------------------
(* Other actors: CommanderHandle::insert_result on the bounded inbox.     *)

OtherReport ==
    \E r \in reportsLeft :
        /\ Room(inbox, InboxCap)
        /\ inbox' = Append(inbox, [type |-> "report", id |-> r])
        /\ reportsLeft' = reportsLeft \ {r}
        /\ UNCHANGED <<api, pc, drained, batch, reply, queue, running, responses,
                       results, pending, failures, restarts>>

---------------------------------------------------------------------------
(* systemd restarts smithd: all in-memory state is gone.                  *)

Restart ==
    /\ restarts < MaxRestarts
    /\ restarts' = restarts + 1
    /\ pc' = "tick"
    /\ drained' = {}
    /\ batch' = <<>>
    /\ inbox' = <<>>
    /\ reply' = <<>>
    /\ queue' = <<>>
    /\ running' = 0
    /\ responses' = <<>>
    /\ results' = [i \in Ids |-> "none"]
    /\ pending' = 0
    /\ UNCHANGED <<api, reportsLeft, failures>>

Next ==
    \/ \E c \in Cmds : UserQueues(c)
    \/ Postman
    \/ Commander
    \/ Executor
    \/ OtherReport
    \/ Restart

\* Tokio runs each task again when it can make progress, and an unbiased
\* `select!` picks among ready branches at random, so no commander branch
\* starves.
Spec ==
    /\ Init /\ [][Next]_vars
    /\ WF_vars(Postman)
    /\ SF_vars(CommanderInbox)
    /\ SF_vars(CommanderSend)
    /\ SF_vars(CommanderResponse)
    /\ WF_vars(Executor)
    /\ WF_vars(OtherReport)

---------------------------------------------------------------------------
(* Properties                                                             *)

\* Somewhere in smithd, or in the api, each fetched command is still on its
\* way to a stored result.
OnDevice(c) ==
    \/ c \in SeqSet(batch)
    \/ \E i \in 1..Len(inbox) : inbox[i].type = "queue" /\ inbox[i].id = c
    \/ c \in SeqSet(queue)
    \/ pending = c
    \/ running = c
    \/ c \in SeqSet(responses)
    \/ results[c] # "none"
    \/ c \in drained
    \/ reply # <<>> /\ c \in reply[1]
    \/ \E i \in 1..Len(inbox) : inbox[i].type = "requeue" /\ c \in inbox[i].ids

\* A fetched command is never forgotten.
NoCommandLost == \A c \in Cmds : api[c] = "fetched" => OnDevice(c)

\* The poll loop never gets stuck: it reaches /home again and again.
KeepsPolling == []<>(pc = "post")

\* Every command a user queues runs and its result reaches the api.
EveryCommandAnswered == \A c \in Cmds : api[c] = "queued" ~> api[c] = "done"
=============================================================================
