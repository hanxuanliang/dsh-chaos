pub(crate) const SCHEMA_VERSION: u32 = 4;

pub(crate) const META_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS collab_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

pub(crate) const SCHEMA_V1: &str = r#"
CREATE TABLE actors (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
  handle TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE agents (
  actor_id TEXT PRIMARY KEY,
  workspace_path TEXT NOT NULL UNIQUE,
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active', 'archived')),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE runtime_bindings (
  agent_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  preset TEXT NOT NULL,
  bound_at_ms INTEGER NOT NULL
);

CREATE TABLE targets (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('channel', 'direct', 'thread')),
  name TEXT NOT NULL,
  parent_target_id TEXT,
  root_message_id TEXT,
  created_by TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  archived_at_ms INTEGER
);

CREATE TABLE memberships (
  target_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
  joined_at_ms INTEGER NOT NULL,
  left_at_ms INTEGER,
  PRIMARY KEY (target_id, actor_id)
);

CREATE TABLE messages (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  target_id TEXT NOT NULL,
  author_id TEXT NOT NULL,
  client_request_id TEXT NOT NULL,
  body_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  UNIQUE (author_id, client_request_id)
);

CREATE INDEX messages_target_seq ON messages (target_id, seq);

CREATE TABLE deliveries (
  id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL,
  message_seq INTEGER NOT NULL,
  target_id TEXT NOT NULL,
  recipient_id TEXT NOT NULL,
  committed_at_ms INTEGER NOT NULL,
  notified_at_ms INTEGER,
  notified_generation INTEGER,
  checked_at_ms INTEGER,
  model_seen_at_ms INTEGER,
  seen_generation INTEGER,
  seen_session_id TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  next_retry_at_ms INTEGER,
  last_error TEXT,
  UNIQUE (message_id, recipient_id)
);

CREATE INDEX deliveries_recipient_seen_seq
  ON deliveries (recipient_id, model_seen_at_ms, message_seq);

CREATE TABLE inbox_batches (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  generation INTEGER NOT NULL,
  checked_at_ms INTEGER NOT NULL,
  model_seen_at_ms INTEGER
);

CREATE TABLE inbox_batch_items (
  batch_id TEXT NOT NULL,
  delivery_id TEXT NOT NULL,
  PRIMARY KEY (batch_id, delivery_id)
);

CREATE TABLE agent_wake_state (
  agent_id TEXT PRIMARY KEY,
  pending_seq INTEGER NOT NULL,
  notified_seq INTEGER NOT NULL DEFAULT 0,
  notified_generation INTEGER NOT NULL DEFAULT 0,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  next_retry_at_ms INTEGER,
  last_error TEXT
);

CREATE TABLE target_counters (
  target_id TEXT PRIMARY KEY,
  next_task_number INTEGER NOT NULL CHECK (next_task_number > 0)
);

CREATE TABLE tasks (
  message_id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL,
  number INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('todo', 'in_progress', 'in_review', 'done')),
  assignee_id TEXT,
  version INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  UNIQUE (target_id, number)
);

CREATE TABLE task_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  from_status TEXT,
  to_status TEXT,
  from_assignee_id TEXT,
  to_assignee_id TEXT,
  task_version INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE read_cursors (
  actor_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  last_read_seq INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  PRIMARY KEY (actor_id, target_id)
);
"#;

pub(crate) const SCHEMA_V2: &str = r#"
CREATE UNIQUE INDEX runtime_bindings_session_id
  ON runtime_bindings (session_id);
"#;

pub(crate) const SCHEMA_V3: &str = r#"
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
"#;

pub(crate) const SCHEMA_V4: &str = r#"
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
"#;
