# dsh-chaos

Peer multi-Agent collaboration for DeepSeek Harness.

The first implementation slice contains:

- a Rust collaboration core backed by a local Turso database;
- atomic Message + recipient Delivery + wake-watermark commits;
- idempotent sends, Task claim concurrency, inbox batches, and runtime-generation fencing;
- a coarse-grained napi-rs feature in `dsh-chaos-core`;
- a DSH `CollabService` host adapter.

The Channel/Thread UI, DSH Agent runtime manager, delivery wake bridge, and model tools are the next vertical slices. See `RAFT-TO-DSH-MULTI-AGENT.md` for the design and boundaries.
