# dsh-chaos

Peer multi-Agent collaboration for DeepSeek Harness.

The current implementation contains:

- a Rust collaboration core backed by a local Turso database;
- atomic Message + recipient Delivery + wake-watermark commits;
- idempotent sends, Task claim concurrency, inbox batches, and runtime-generation fencing;
- a coarse-grained napi-rs feature in `dsh-chaos-core`;
- a DSH `CollabService` host adapter;
- unique Direct targets plus parent-authorized Threads whose ordinary Delivery/wake remains follower-scoped while authorized open views stay realtime;
- a recipient-snapshotted durable change ledger with bootstrap snapshots, decimal cursors, seven-day retention, and explicit full-snapshot resync when a cursor falls outside the retained range;
- a `RuntimeManager` that creates/resumes/resets owned top-level `AgentHandle`s, composes each through the official DSH Agent Preset roster, and binds it to a stable collab Agent;
- a level-triggered, generation-fenced `DeliveryBridge` that sends body-free wake notices;
- Agent-scoped message tools plus Task create/claim/list/unclaim/status tools whose actor identity comes from `exec.agent`;
- a fixed-principal, loopback-authorized DSH Connection RPC channel and recipient-filtered SSE change stream with durable `Last-Event-ID` replay;
- an official Sidebar footer entry with a pending-Task badge and Quick Peek, opening a responsive three-column collaboration workspace for Channel membership, Direct, Thread Follow/Unfollow, Message, and Task operations;
- an Agent Settings surface for preset-aware creation, the Agent's one bound Session, and read-only browsing of its fixed `$DSH_HOME/agents/<id>/` Workspace;
- authoritative RPC reads after SSE invalidations, with Rust/Turso snapshots as the source of truth for Thread follows and every other projected state.

The browser panel is an initial functional slice rather than a complete Channel product. Remote access is intentionally loopback-only because DSH is a local product; local tabs and browsers share one fixed Web User. SSE is an invalidation projection, while Rust/Turso remains authoritative. See `RAFT-TO-DSH-MULTI-AGENT.md` for the design and boundaries.
