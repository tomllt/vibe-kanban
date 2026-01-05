import { useMemo } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Loader } from '@/components/ui/loader';

import { useProject } from '@/contexts/ProjectContext';
import { useProjectTasks } from '@/hooks/useProjectTasks';
import { openTaskForm } from '@/lib/openTaskForm';
import { paths } from '@/lib/paths';

import type { TaskType, TaskWithAttemptStatus } from 'shared/types';

type Task = TaskWithAttemptStatus;

function typeLabel(taskType: TaskType): string {
  switch (taskType) {
    case 'epic':
      return 'Epic';
    case 'feature':
      return 'Feature';
    case 'story':
      return 'Story';
    default:
      return 'Task';
  }
}

export function ProjectBacklog() {
  const { t } = useTranslation(['tasks', 'common']);
  const navigate = useNavigate();
  const { projectId, project, isLoading: projectLoading, error } = useProject();
  const { tasks, isLoading: tasksLoading } = useProjectTasks(projectId || '');

  const { epics, backlogByEpicId, epicById } = useMemo(() => {
    const epicById: Record<string, Task> = {};
    const epics: Task[] = [];
    for (const task of tasks) {
      if (task.task_type === 'epic') {
        epicById[task.id] = task;
        epics.push(task);
      }
    }

    const backlogItems = tasks.filter((task) => {
      if (task.task_type === 'epic') return false;
      if (task.status === 'done' || task.status === 'cancelled') return false;
      return task.sprint_id == null;
    });

    const backlogByEpicId: Record<string, Task[]> = {};
    for (const task of backlogItems) {
      const key = task.epic_id ?? 'none';
      backlogByEpicId[key] ||= [];
      backlogByEpicId[key].push(task);
    }

    Object.values(backlogByEpicId).forEach((list) => {
      list.sort((a, b) => {
        const rank = (type: TaskType) =>
          type === 'feature' ? 0 : type === 'story' ? 1 : 2;
        return (
          rank(a.task_type) - rank(b.task_type) ||
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
        );
      });
    });

    epics.sort(
      (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    );

    return { epics, backlogByEpicId, epicById };
  }, [tasks]);

  const isLoading = projectLoading || tasksLoading;

  if (error) {
    return (
      <div className="p-4">
        <Card>
          <CardContent className="py-6">
            <p className="text-destructive">{error.message}</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  if (isLoading) {
    return <Loader message={t('loading')} size={32} className="py-8" />;
  }

  if (!projectId) {
    return null;
  }

  const handleCreate = () => openTaskForm({ mode: 'create', projectId });

  const laneOrder = [
    ...epics.map((e) => e.id),
    ...(backlogByEpicId.none?.length ? ['none'] : []),
  ];

  return (
    <div className="p-4 max-w-6xl mx-auto space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-lg font-medium truncate">
            {project?.name ?? 'Project'} – Backlog
          </h1>
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Link className="hover:underline" to={paths.projectTasks(projectId)}>
              Board
            </Link>
            <span>·</span>
            <Link
              className="hover:underline"
              to={paths.projectSprintPlanning(projectId)}
            >
              Sprint planning
            </Link>
          </div>
        </div>
        <Button onClick={handleCreate}>
          <Plus className="h-4 w-4 mr-2" />
          {t('actions.addTask', { defaultValue: 'Add task' })}
        </Button>
      </div>

      {laneOrder.length === 0 ? (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-muted-foreground">{t('empty.noTasks')}</p>
            <Button className="mt-4" onClick={handleCreate}>
              <Plus className="h-4 w-4 mr-2" />
              {t('empty.createFirst')}
            </Button>
          </CardContent>
        </Card>
      ) : (
        laneOrder.map((laneId) => {
          const epic = laneId === 'none' ? null : epicById[laneId];
          const items = backlogByEpicId[laneId] ?? [];
          if (!epic && laneId !== 'none') return null;

          const storyPoints = items.reduce((sum, task) => {
            if (task.story_points == null) return sum;
            return sum + task.story_points;
          }, 0);

          return (
            <Card key={laneId}>
              <CardContent className="py-4 space-y-3">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <h2 className="font-medium truncate">
                        {epic ? epic.title : 'No epic'}
                      </h2>
                      <Badge variant="secondary">{items.length}</Badge>
                      {storyPoints > 0 ? (
                        <Badge variant="outline">{storyPoints} pts</Badge>
                      ) : null}
                    </div>
                    {epic?.description ? (
                      <p className="text-sm text-muted-foreground line-clamp-1">
                        {epic.description}
                      </p>
                    ) : null}
                  </div>
                </div>

                {items.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    No backlog items
                  </p>
                ) : (
                  <div className="space-y-2">
                    {items.map((task) => (
                      <button
                        key={task.id}
                        type="button"
                        className="w-full text-left rounded border bg-background hover:bg-muted/50 px-3 py-2"
                        onClick={() => navigate(paths.task(projectId, task.id))}
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="flex items-center gap-2">
                              <Badge variant="secondary">
                                {typeLabel(task.task_type)}
                              </Badge>
                              {task.story_points != null ? (
                                <Badge variant="outline">
                                  {task.story_points} pts
                                </Badge>
                              ) : null}
                              <span className="truncate">{task.title}</span>
                            </div>
                            {task.description ? (
                              <p className="text-sm text-muted-foreground line-clamp-1">
                                {task.description}
                              </p>
                            ) : null}
                          </div>
                          <span className="text-xs text-muted-foreground shrink-0">
                            {task.status}
                          </span>
                        </div>
                      </button>
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>
          );
        })
      )}
    </div>
  );
}

