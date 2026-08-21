ALTER TABLE agents ADD COLUMN charter_json TEXT NOT NULL
  DEFAULT '{"schemaVersion":1,"summary":"","capabilities":[],"constraints":[]}';
ALTER TABLE agents ADD COLUMN profile_version INTEGER NOT NULL DEFAULT 1
  CHECK (profile_version > 0);

-- Extend the closed change-kind vocabulary without creating a second profile
-- audit ledger. Consumers invalidate and re-read the authoritative Profile.
ALTER TABLE change_events RENAME TO change_events_v5;

CREATE TABLE change_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK (kind IN (
    'actor_created',
    'agent_profile_changed',
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
FROM change_events_v5
ORDER BY seq;

DROP TABLE change_events_v5;
