PRAGMA foreign_keys = ON;

-- Task status events for analytics (CFD, burndown, cycle time)
CREATE TABLE IF NOT EXISTS task_status_events (
    id          BLOB PRIMARY KEY,
    task_id     BLOB NOT NULL,
    project_id  BLOB NOT NULL,
    status      TEXT NOT NULL
                   CHECK (status IN ('todo','inprogress','done','cancelled','inreview')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_status_events_project_created
    ON task_status_events (project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_task_status_events_task_created
    ON task_status_events (task_id, created_at);

-- Backfill one status event per existing task (best effort).
-- Note: historical transitions prior to this migration are not recoverable.
INSERT INTO task_status_events (id, task_id, project_id, status, created_at)
SELECT
    randomblob(16),
    t.id,
    t.project_id,
    t.status,
    t.created_at
FROM tasks t
WHERE NOT EXISTS (
    SELECT 1 FROM task_status_events e WHERE e.task_id = t.id LIMIT 1
);

-- Keep task_status_events up to date automatically.
DROP TRIGGER IF EXISTS tasks_ai_status_event;
CREATE TRIGGER tasks_ai_status_event
AFTER INSERT ON tasks
BEGIN
    INSERT INTO task_status_events (id, task_id, project_id, status, created_at)
    VALUES (randomblob(16), NEW.id, NEW.project_id, NEW.status, NEW.created_at);
END;

DROP TRIGGER IF EXISTS tasks_au_status_change_event;
CREATE TRIGGER tasks_au_status_change_event
AFTER UPDATE OF status ON tasks
WHEN OLD.status != NEW.status
BEGIN
    INSERT INTO task_status_events (id, task_id, project_id, status, created_at)
    VALUES (randomblob(16), NEW.id, NEW.project_id, NEW.status, datetime('now', 'subsec'));
END;
