CREATE UNIQUE INDEX targets_root_message_id
  ON targets (root_message_id);

CREATE TABLE direct_pairs (
  target_id TEXT PRIMARY KEY,
  actor_low_id TEXT NOT NULL,
  actor_high_id TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  CHECK (actor_low_id < actor_high_id),
  UNIQUE (actor_low_id, actor_high_id)
);

CREATE TABLE thread_follows (
  thread_target_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  followed_at_ms INTEGER NOT NULL,
  unfollowed_at_ms INTEGER,
  PRIMARY KEY (thread_target_id, actor_id)
);

CREATE INDEX thread_follows_active
  ON thread_follows (thread_target_id, unfollowed_at_ms, actor_id);
