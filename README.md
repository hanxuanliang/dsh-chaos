# dsh-chaos

Peer multi-Agent collaboration for DeepSeek Harness.

The current implementation contains:

- a Rust collaboration core backed by a local Turso database;
- atomic Message + recipient Delivery + wake-watermark commits;
- idempotent sends, Task claim concurrency, inbox batches, and runtime-generation fencing;
- a coarse-grained napi-rs feature in `dsh-chaos-core`;
- a DSH `CollabService` host adapter;
- unique Direct targets plus parent-authorized, follower-delivered Threads;
- a recipient-snapshotted durable change ledger with bootstrap snapshots and decimal cursors;
- a `RuntimeManager` that creates/resumes/resets owned top-level `AgentHandle`s and binds them to stable collab Agents;
- a level-triggered, generation-fenced `DeliveryBridge` that sends body-free wake notices;
- Agent-scoped message tools plus Task create/claim/list/unclaim/status tools whose actor identity comes from `exec.agent`;
- a fixed-principal, loopback-authorized DSH Connection RPC channel and recipient-filtered SSE change stream with durable `Last-Event-ID` replay;
- an additive browser collaboration panel for Channel membership, Direct, Thread, Message, and Task operations, refreshed through authoritative RPC reads after SSE invalidations.

The browser panel is an initial functional slice rather than a complete Channel product. Remote access is intentionally loopback-only until DSH has a real authenticated multi-user browser principal; SSE is an invalidation projection, while Rust/Turso remains authoritative. See `RAFT-TO-DSH-MULTI-AGENT.md` for the design and boundaries.
