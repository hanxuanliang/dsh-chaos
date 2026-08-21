CREATE TABLE activity_inbox_done (
  actor_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  done_through_seq INTEGER NOT NULL CHECK (done_through_seq > 0),
  updated_at_ms INTEGER NOT NULL,
  PRIMARY KEY (actor_id, target_id)
);

-- SQLite cannot extend a CHECK constraint in place. Rebuild the durable
-- change ledger so actor-private Activity Done mutations have an honest kind
-- and can invalidate other tabs through the existing SSE replay path.
ALTER TABLE change_events RENAME TO change_events_v4;

CREATE TABLE change_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK (kind IN (
    'actor_created',
    'target_created',
    'membership_changed',
    'thread_follow_changed',
    'message_created',
    'task_created',
    'task_updated',
    'activity_done_changed'
  )),
  target_id TEXT,
  entity_id TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

INSERT INTO change_events (seq, kind, target_id, entity_id, created_at_ms)
SELECT seq, kind, target_id, entity_id, created_at_ms
FROM change_events_v4
ORDER BY seq;

DROP TABLE change_events_v4;
