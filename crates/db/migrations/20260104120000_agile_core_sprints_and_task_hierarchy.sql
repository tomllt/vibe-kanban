PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS sprints (
    id         BLOB PRIMARY KEY,
    project_id BLOB NOT NULL,
    name       TEXT NOT NULL,
    goal       TEXT,
    start_date TEXT,
    end_date   TEXT,
    status     TEXT NOT NULL DEFAULT 'planned'
                   CHECK (status IN ('planned', 'active', 'closed')),
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sprints_project_id ON sprints(project_id);
CREATE INDEX IF NOT EXISTS idx_sprints_project_status ON sprints(project_id, status);

ALTER TABLE tasks ADD COLUMN sprint_id BLOB REFERENCES sprints(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN task_type TEXT NOT NULL DEFAULT 'task'
                      CHECK (task_type IN ('epic', 'feature', 'story', 'task'));
ALTER TABLE tasks ADD COLUMN epic_id BLOB REFERENCES tasks(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN parent_task_id BLOB REFERENCES tasks(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN story_points INTEGER CHECK (story_points IS NULL OR story_points >= 0);

CREATE INDEX IF NOT EXISTS idx_tasks_sprint_id ON tasks(sprint_id);
CREATE INDEX IF NOT EXISTS idx_tasks_task_type ON tasks(task_type);
CREATE INDEX IF NOT EXISTS idx_tasks_epic_id ON tasks(epic_id);
CREATE INDEX IF NOT EXISTS idx_tasks_parent_task_id ON tasks(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_tasks_project_sprint_id ON tasks(project_id, sprint_id);

