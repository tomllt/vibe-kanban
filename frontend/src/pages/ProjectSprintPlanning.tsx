import { useCallback, useMemo, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Loader } from '@/components/ui/loader';
import { Badge } from '@/components/ui/badge';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  type DragEndEvent,
  KanbanBoard,
  KanbanCards,
  KanbanHeader,
  KanbanProvider,
} from '@/components/ui/shadcn-io/kanban';

import { TaskCard } from '@/components/tasks/TaskCard';
import { useProject } from '@/contexts/ProjectContext';
import { useProjectTasks } from '@/hooks/useProjectTasks';
import { openTaskForm } from '@/lib/openTaskForm';
import { paths } from '@/lib/paths';
import { sprintsApi } from '@/lib/api';

import type { Sprint, TaskType, TaskWithAttemptStatus } from 'shared/types';

type Task = TaskWithAttemptStatus;

function laneRank(type: TaskType): number {
  if (type === 'feature') return 0;
  if (type === 'story') return 1;
  return 2;
}

function Lane({
  title,
  backlog,
  sprintItems,
  onDragEnd,
  projectId,
  onViewTaskDetails,
}: {
  title: string;
  backlog: Task[];
  sprintItems: Task[];
  onDragEnd: (event: DragEndEvent) => void;
  projectId: string;
  onViewTaskDetails: (task: Task) => void;
}) {
  return (
    <Card>
      <CardContent className="py-4 space-y-3">
        <div className="flex items-center justify-between gap-3">
          <h2 className="font-medium truncate">{title}</h2>
          <div className="flex items-center gap-2">
            <Badge variant="secondary">Backlog: {backlog.length}</Badge>
            <Badge variant="secondary">Sprint: {sprintItems.length}</Badge>
          </div>
        </div>

        <KanbanProvider onDragEnd={onDragEnd} className="rounded border overflow-hidden">
          <KanbanBoard id="backlog" className="bg-background">
            <KanbanHeader>
              <div className="sticky top-0 z-20 flex items-center justify-between p-3 border-b bg-background">
                <span className="text-sm font-medium">Backlog</span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => openTaskForm({ mode: 'create', projectId })}
                >
                  <Plus className="h-4 w-4 mr-2" />
                  Add
                </Button>
              </div>
            </KanbanHeader>
            <KanbanCards>
              {backlog.map((task, index) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  index={index}
                  status="backlog"
                  onViewDetails={onViewTaskDetails}
                  projectId={projectId}
                />
              ))}
            </KanbanCards>
          </KanbanBoard>

          <KanbanBoard id="sprint" className="bg-background">
            <KanbanHeader>
              <div className="sticky top-0 z-20 flex items-center justify-between p-3 border-b bg-background">
                <span className="text-sm font-medium">Sprint</span>
              </div>
            </KanbanHeader>
            <KanbanCards>
              {sprintItems.map((task, index) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  index={index}
                  status="sprint"
                  onViewDetails={onViewTaskDetails}
                  projectId={projectId}
                />
              ))}
            </KanbanCards>
          </KanbanBoard>
        </KanbanProvider>
      </CardContent>
    </Card>
  );
}

export function ProjectSprintPlanning() {
  const { t } = useTranslation(['tasks', 'common']);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { projectId, project, isLoading: projectLoading, error } = useProject();
  const { tasks, tasksById, isLoading: tasksLoading } = useProjectTasks(
    projectId || ''
  );

  const [isCreateSprintOpen, setIsCreateSprintOpen] = useState(false);
  const [newSprintName, setNewSprintName] = useState('');
  const [newSprintGoal, setNewSprintGoal] = useState('');

  const { data: sprints = [], isLoading: sprintsLoading } = useQuery({
    queryKey: ['sprints', projectId],
    enabled: Boolean(projectId),
    queryFn: () => sprintsApi.list(projectId!),
  });

  const sprintId = searchParams.get('sprintId');
  const selectedSprint: Sprint | null =
    sprintId && sprints.length
      ? sprints.find((s) => s.id === sprintId) ?? null
      : null;

  const createSprintMutation = useMutation({
    mutationFn: async () => {
      if (!projectId) throw new Error('Missing project');
      return sprintsApi.create({
        project_id: projectId,
        name: newSprintName.trim(),
        goal: newSprintGoal.trim() || null,
        start_date: null,
        end_date: null,
        status: 'planned',
      });
    },
    onSuccess: (created) => {
      queryClient.invalidateQueries({ queryKey: ['sprints', projectId] });
      setIsCreateSprintOpen(false);
      setNewSprintName('');
      setNewSprintGoal('');
      const next = new URLSearchParams(searchParams);
      next.set('sprintId', created.id);
      setSearchParams(next, { replace: true });
    },
  });

  const assignMutation = useMutation({
    mutationFn: async (taskId: string) => {
      if (!selectedSprint) throw new Error('No sprint selected');
      return sprintsApi.assign(selectedSprint.id, [taskId]);
    },
  });

  const unassignMutation = useMutation({
    mutationFn: async (taskId: string) => {
      if (!selectedSprint) throw new Error('No sprint selected');
      return sprintsApi.unassign(selectedSprint.id, [taskId]);
    },
  });

  const { epics, lanes } = useMemo(() => {
    const epics = tasks
      .filter((t) => t.task_type === 'epic')
      .sort(
        (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      );

    const laneIds = new Set<string>();
    for (const task of tasks) {
      if (task.task_type === 'epic') continue;
      if (task.epic_id) laneIds.add(task.epic_id);
      else laneIds.add('none');
    }

    const orderedLaneIds = [
      ...epics.map((e) => e.id).filter((id) => laneIds.has(id)),
      ...(laneIds.has('none') ? ['none'] : []),
    ];

    return { epics, lanes: orderedLaneIds };
  }, [tasks]);

  const isLoading = projectLoading || tasksLoading || sprintsLoading;

  const handleSprintChange = useCallback(
    (value: string) => {
      const next = new URLSearchParams(searchParams);
      if (value === 'none') {
        next.delete('sprintId');
      } else {
        next.set('sprintId', value);
      }
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams]
  );

  const handleCreateTask = useCallback(() => {
    if (!projectId) return;
    openTaskForm({ mode: 'create', projectId });
  }, [projectId]);

  const handleViewTaskDetails = useCallback(
    (task: Task) => {
      if (!projectId) return;
      navigate(paths.task(projectId, task.id));
    },
    [navigate, projectId]
  );

  const handleLaneDragEnd = useCallback(
    async (event: DragEndEvent) => {
      if (!selectedSprint) return;
      const { active, over } = event;
      if (!over || !active.id) return;

      const taskId = String(active.id);
      const task = tasksById[taskId];
      if (!task) return;

      if (over.id === 'sprint') {
        if (task.sprint_id === selectedSprint.id) return;
        try {
          await assignMutation.mutateAsync(taskId);
        } catch (err) {
          console.error('Failed to assign task to sprint:', err);
        }
      } else if (over.id === 'backlog') {
        if (task.sprint_id !== selectedSprint.id) return;
        try {
          await unassignMutation.mutateAsync(taskId);
        } catch (err) {
          console.error('Failed to unassign task from sprint:', err);
        }
      }
    },
    [selectedSprint, tasksById, assignMutation, unassignMutation]
  );

  const lanesData = useMemo(() => {
    if (!selectedSprint) return [];
    const epicById = Object.fromEntries(epics.map((e) => [e.id, e]));

    return lanes.map((laneId) => {
      const laneTitle = laneId === 'none' ? 'No epic' : epicById[laneId]?.title ?? 'Epic';

      const isInLane = (task: Task) =>
        task.task_type !== 'epic' && (task.epic_id ?? 'none') === laneId;

      const backlog = tasks
        .filter(
          (t) =>
            isInLane(t) &&
            t.sprint_id == null &&
            t.status !== 'done' &&
            t.status !== 'cancelled'
        )
        .sort((a, b) => {
          return (
            laneRank(a.task_type) - laneRank(b.task_type) ||
            new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
          );
        });

      const sprintItems = tasks
        .filter((t) => isInLane(t) && t.sprint_id === selectedSprint.id)
        .sort((a, b) => {
          return (
            laneRank(a.task_type) - laneRank(b.task_type) ||
            new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
          );
        });

      return { laneId, laneTitle, backlog, sprintItems };
    });
  }, [selectedSprint, tasks, lanes, epics]);

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

  if (!projectId) return null;

  return (
    <div className="p-4 max-w-6xl mx-auto space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-lg font-medium truncate">
            {project?.name ?? 'Project'} – Sprint planning
          </h1>
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Link className="hover:underline" to={paths.projectTasks(projectId)}>
              Board
            </Link>
            <span>·</span>
            <Link className="hover:underline" to={paths.projectBacklog(projectId)}>
              Backlog
            </Link>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Button variant="outline" onClick={() => setIsCreateSprintOpen(true)}>
            New sprint
          </Button>
          <Button onClick={handleCreateTask}>
            <Plus className="h-4 w-4 mr-2" />
            Add task
          </Button>
        </div>
      </div>

      <div className="flex items-end gap-3">
        <div className="min-w-[260px]">
          <Label>Sprint</Label>
          <Select
            value={selectedSprint?.id ?? 'none'}
            onValueChange={handleSprintChange}
          >
            <SelectTrigger>
              <SelectValue placeholder="Select a sprint" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">No sprint</SelectItem>
              {sprints.map((s) => (
                <SelectItem key={s.id} value={s.id}>
                  {s.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {selectedSprint ? (
          <div className="text-sm text-muted-foreground">
            {selectedSprint.status} ·{' '}
            {selectedSprint.goal ? selectedSprint.goal : 'No goal'}
          </div>
        ) : null}
      </div>

      {!selectedSprint ? (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-muted-foreground">
              Select a sprint to start planning
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {lanesData.length === 0 ? (
            <Card>
              <CardContent className="py-8 text-center">
                <p className="text-muted-foreground">
                  No backlog items to plan
                </p>
              </CardContent>
            </Card>
          ) : (
            lanesData.map((lane) => (
              <Lane
                key={lane.laneId}
                title={lane.laneTitle}
                backlog={lane.backlog}
                sprintItems={lane.sprintItems}
                onDragEnd={handleLaneDragEnd}
                projectId={projectId}
                onViewTaskDetails={handleViewTaskDetails}
              />
            ))
          )}
        </div>
      )}

      <Dialog open={isCreateSprintOpen} onOpenChange={setIsCreateSprintOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New sprint</DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="sprint-name">Name</Label>
              <Input
                id="sprint-name"
                value={newSprintName}
                onChange={(e) => setNewSprintName(e.target.value)}
                placeholder="Sprint 1"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="sprint-goal">Goal (optional)</Label>
              <Input
                id="sprint-goal"
                value={newSprintGoal}
                onChange={(e) => setNewSprintGoal(e.target.value)}
                placeholder="Ship sprint planning UI"
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setIsCreateSprintOpen(false)}
            >
              Cancel
            </Button>
            <Button
              disabled={!newSprintName.trim() || createSprintMutation.isPending}
              onClick={() => createSprintMutation.mutate()}
            >
              Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
