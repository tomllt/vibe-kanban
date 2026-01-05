-- Add sprint support + task completion timestamps for sprint-based release notes

-- 1) Track when a task was marked done (used for sprint membership)
ALTER TABLE tasks ADD COLUMN done_at TEXT;

CREATE INDEX IF NOT EXISTS idx_tasks_project_done_at
    ON tasks(project_id, done_at);

-- 2) Sprints are date-ranged groupings per project
CREATE TABLE sprints (
    id         BLOB PRIMARY KEY,
    project_id BLOB NOT NULL,
    name       TEXT NOT NULL,
    start_at   TEXT NOT NULL,
    end_at     TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sprints_project_id
    ON sprints(project_id);

CREATE INDEX IF NOT EXISTS idx_sprints_project_start_end
    ON sprints(project_id, start_at, end_at);

