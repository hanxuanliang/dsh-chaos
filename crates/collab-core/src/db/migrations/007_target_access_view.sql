-- Single owner of the permission-inheritance projection: one row per
-- (active target, active member) pair, with each Thread resolved to the
-- parent target whose membership governs it. The view carries visibility
-- joins only; role and guard decisions stay in the Rust evidence chain.
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
  AND target.archived_at_ms IS NULL;
