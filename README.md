# dsh-chaos

Peer multi-Agent collaboration for DeepSeek Harness.

The current implementation contains:

- a Rust collaboration core backed by a local Turso database;
- atomic Message + recipient Delivery + wake-watermark commits;
- idempotent sends, Task claim concurrency, inbox batches, and runtime-generation fencing;
- a coarse-grained napi-rs feature in `dsh-chaos-core`;
- a DSH `CollabService` host adapter;
- a `RuntimeManager` that creates/resumes/resets owned top-level `AgentHandle`s and binds them to stable collab Agents;
- a level-triggered, generation-fenced `DeliveryBridge` that sends body-free wake notices;
- Agent-scoped `message_check` and `message_send` tools whose author identity comes from `exec.agent`.

Direct/Thread product behavior, Task tools, Remote API, and the Channel UI remain future vertical slices. See `RAFT-TO-DSH-MULTI-AGENT.md` for the design and boundaries.
