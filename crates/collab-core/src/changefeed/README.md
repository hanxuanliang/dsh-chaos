# Changefeed

The changefeed tells connected clients which collaboration data may have changed. It is a durable invalidation log, not a second copy of the collaboration model.

Authoritative state remains in tables such as `messages`, `deliveries`, `memberships`, and `tasks`. A changefeed row carries only enough information for a client to decide what to read again: a monotonically increasing sequence, a notification kind, an optional target ID, an entity ID, and a timestamp.

## Write path

Every user-visible mutation appends its notification in the same database transaction as the authoritative write:

```text
Message, Task, Membership, or Profile mutation
                    |
                    +-- change_events(seq, kind, target_id, entity_id)
                    +-- change_recipients(change_seq, actor_id)
```

This atomic write is the reason the changefeed is durable. If the process stops after the transaction commits but before an SSE frame is sent, a reconnecting client can still replay the committed notification.

`change_events` provides one global order. `change_recipients` limits each notification to actors who are allowed to observe the affected projection. The recipient snapshot drives UI synchronization only; it does not create Message delivery or Agent attention.

## Replay and resynchronization

A client first loads a bootstrap snapshot and remembers its cursor. The SSE endpoint then calls `list_changes(actor_id, cursor, limit)` and emits the actor's later notifications in sequence order.

The notification payload is deliberately small. On receipt, the client reloads the relevant projection through the normal RPC API. It must not treat the notification as current business state.

The host currently retains seven days of notifications and prunes once per hour. Pruning advances a durable retention floor. A cursor below that floor, or ahead of the latest known sequence, returns `change_cursor_resync_required`; the client discards incremental assumptions and loads a fresh snapshot.

## What the changefeed is not

- It is not Message Delivery. Delivery records which Agent received a Message and tracks notified, checked, and model-seen state.
- It is not an audit log. Notifications do not contain complete before-and-after values and cannot reconstruct domain history. Task history, for example, belongs in `task_events`.
- It is not a domain event bus. The rows are client synchronization hints backed by durable identifiers.
- It is not a source of truth. Clients always re-read authoritative projections after invalidation.

## Correctness rules

The changefeed relies on a small set of invariants:

1. Append the authoritative mutation and its notification in one transaction.
2. Compute recipients from the authorization state observed by that transaction.
3. Keep sequence numbers monotonic and never reuse a pruned cursor.
4. Read multi-query snapshots and replay bounds from one database snapshot so the returned cursor cannot be older than the returned state.
5. Fall back to a full snapshot whenever incremental replay cannot be proven complete.

The fourth rule is an intended contract that the current implementation still needs to enforce with explicit deferred read transactions across multiple database connections.

## Module direction

The current `mod.rs` still contains bootstrap snapshot reads, notification writes, cursor replay, and retention. Those responsibilities should separate as the core is reorganized:

```text
changefeed/
  mod.rs
  writer.rs       # append notifications and recipient rows
  replay.rs       # cursor validation and actor-filtered replay
  retention.rs    # retention floor, pruning, and resync decisions
snapshot.rs       # bootstrap projection, outside the changefeed
```

Public RPC names remain `snapshot`, `list_changes`, and `prune_changes_before` for compatibility. The internal module name describes the mechanism without forcing a wire-level rename.
