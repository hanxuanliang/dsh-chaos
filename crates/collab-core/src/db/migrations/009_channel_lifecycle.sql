ALTER TABLE targets ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE targets ADD COLUMN version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0);
ALTER TABLE targets ADD COLUMN updated_at_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE targets ADD COLUMN deleted_at_ms INTEGER;

UPDATE targets SET updated_at_ms = created_at_ms WHERE updated_at_ms = 0;

-- Read visibility includes archived targets. Command guards in Rust decide
-- whether a visible target is writable. Deleted targets are exact-history
-- only and never appear in normal snapshots.
DROP VIEW v_target_access;
CREATE VIEW v_target_access AS
SELECT
  target.id   AS target_id,
  target.kind AS target_kind,
  CASE WHEN target.kind = 'thread'
       THEN target.parent_target_id ELSE target.id END AS permission_target_id,
  membership.actor_id AS actor_id,
  membership.role     AS role
FROM targets target
JOIN memberships membership
  ON membership.target_id = (CASE WHEN target.kind = 'thread'
                                  THEN target.parent_target_id ELSE target.id END)
WHERE membership.left_at_ms IS NULL
  AND target.deleted_at_ms IS NULL;

-- Extend the closed change-kind vocabulary for Channel metadata and
-- lifecycle invalidations. Recipients remain snapshotted before deletion.
ALTER TABLE change_events RENAME TO change_events_v8;

CREATE TABLE change_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK (kind IN (
    'actor_created',
    'agent_profile_changed',
    'target_created',
    'target_changed',
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
FROM change_events_v8
ORDER BY seq;

DROP TABLE change_events_v8;
