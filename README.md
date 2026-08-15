# dsh-chaos

Peer multi-Agent collaboration for DeepSeek Harness.

The current implementation contains:

- a Rust collaboration core backed by a local Turso database;
- atomic Message + recipient Delivery + wake-watermark commits;
- idempotent sends, Task claim concurrency, inbox batches, and runtime-generation fencing;
- a coarse-grained napi-rs feature in `dsh-chaos-core`;
- a DSH `CollabService` host adapter;
- unique Direct targets plus parent-authorized, follower-delivered Threads;
- a `RuntimeManager` that creates/resumes/resets owned top-level `AgentHandle`s and binds them to stable collab Agents;
- a level-triggered, generation-fenced `DeliveryBridge` that sends body-free wake notices;
- Agent-scoped `message_check`, `message_read`, `message_send`, `task_create`, and `task_claim` tools whose actor identity comes from `exec.agent`.

The Remote API, Web UI, and the remaining Task lifecycle operations remain future vertical slices. See `RAFT-TO-DSH-MULTI-AGENT.md` for the design and boundaries.
