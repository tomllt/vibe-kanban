-- Track environment promotions (e.g. merges/pushes to staging/prod branches) per task.
CREATE TABLE IF NOT EXISTS environment_promotions (
  id              TEXT PRIMARY KEY NOT NULL,
  task_id          TEXT NOT NULL,
  workspace_id     TEXT,
  environment      TEXT NOT NULL,
  status           TEXT NOT NULL,
  target_branch    TEXT NOT NULL,
  merge_commit_sha TEXT,
  message          TEXT,
  created_at       TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
  updated_at       TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
  FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_environment_promotions_task_env_created_at
  ON environment_promotions (task_id, environment, created_at DESC);

