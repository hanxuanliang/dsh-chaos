CREATE TABLE change_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK (kind IN (
    'actor_created',
    'target_created',
    'membership_changed',
    'thread_follow_changed',
    'message_created',
    'task_created',
    'task_updated'
  )),
  target_id TEXT,
  entity_id TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE change_recipients (
  change_seq INTEGER NOT NULL,
  actor_id TEXT NOT NULL,
  PRIMARY KEY (change_seq, actor_id)
);

CREATE INDEX change_recipients_actor_seq
  ON change_recipients (actor_id, change_seq);
