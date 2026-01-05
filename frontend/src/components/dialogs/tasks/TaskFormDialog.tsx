import { useEffect, useCallback, useRef, useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import { useDropzone } from 'react-dropzone';
import { useForm, useStore } from '@tanstack/react-form';
import { Image as ImageIcon } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import WYSIWYGEditor from '@/components/ui/wysiwyg';
import type { LocalImageMetadata } from '@/components/ui/wysiwyg/context/task-attempt-context';
import BranchSelector from '@/components/tasks/BranchSelector';
import RepoBranchSelector from '@/components/tasks/RepoBranchSelector';
import { ExecutorProfileSelector } from '@/components/settings';
import { useUserSystem } from '@/components/ConfigProvider';
import {
  useTaskImages,
  useImageUpload,
  useTaskMutations,
  useProjectRepos,
  useRepoBranchSelection,
} from '@/hooks';
import { useProjectTasks } from '@/hooks/useProjectTasks';
import {
  useKeySubmitTask,
  useKeySubmitTaskAlt,
  useKeyExit,
  Scope,
} from '@/keyboard';
import { useHotkeysContext } from 'react-hotkeys-hook';
import { cn } from '@/lib/utils';
import type {
  TaskStatus,
  TaskType,
  ExecutorProfileId,
  ImageResponse,
  TaskWithAttemptStatus,
} from 'shared/types';

interface Task {
  id: string;
  project_id: string;
  title: string;
  description: string | null;
  status: TaskStatus;
  sprint_id: string | null;
  task_type: TaskType;
  epic_id: string | null;
  parent_task_id: string | null;
  story_points: number | null;
  parent_workspace_id: string | null;
  shared_task_id: string | null;
  created_at: string;
  updated_at: string;
}

export type TaskFormDialogProps =
  | { mode: 'create'; projectId: string }
  | { mode: 'edit'; projectId: string; task: Task }
  | { mode: 'duplicate'; projectId: string; initialTask: Task }
  | {
      mode: 'subtask';
      projectId: string;
      parentTaskAttemptId: string;
      initialBaseBranch: string;
    };

type RepoBranch = { repoId: string; branch: string };

type TaskFormValues = {
  title: string;
  description: string;
  status: TaskStatus;
  taskType: TaskType;
  epicId: string | null;
  parentTaskId: string | null;
  storyPoints: string;
  executorProfileId: ExecutorProfileId | null;
  repoBranches: RepoBranch[];
  autoStart: boolean;
};

const TaskFormDialogImpl = NiceModal.create<TaskFormDialogProps>((props) => {
  const { mode, projectId } = props;
  const editMode = mode === 'edit';
  const isSubtask = mode === 'subtask';
  const modal = useModal();
  const { t } = useTranslation(['tasks', 'common']);
  const { createTask, createAndStart, updateTask } =
    useTaskMutations(projectId);
  const { system, profiles, loading: userSystemLoading } = useUserSystem();
  const { upload, uploadForTask } = useImageUpload();
  const { enableScope, disableScope } = useHotkeysContext();

  // Local UI state
  const [images, setImages] = useState<ImageResponse[]>([]);
  const [newlyUploadedImageIds, setNewlyUploadedImageIds] = useState<string[]>(
    []
  );
  const [showDiscardWarning, setShowDiscardWarning] = useState(false);
  const forceCreateOnlyRef = useRef(false);

  const { data: taskImages } = useTaskImages(
    editMode ? props.task.id : undefined
  );
  const { data: projectRepos = [] } = useProjectRepos(projectId, {
    enabled: modal.visible,
  });
  const initialBranch =
    mode === 'subtask' ? props.initialBaseBranch : undefined;
  const { configs: repoBranchConfigs, isLoading: branchesLoading } =
    useRepoBranchSelection({
      repos: projectRepos,
      initialBranch,
      enabled: modal.visible && projectRepos.length > 0,
    });

  const defaultRepoBranches = useMemo((): RepoBranch[] => {
    return repoBranchConfigs
      .filter((c) => c.targetBranch !== null)
      .map((c) => ({ repoId: c.repoId, branch: c.targetBranch! }));
  }, [repoBranchConfigs]);

  const { tasks: projectTasks } = useProjectTasks(projectId);
  const projectTasksById = useMemo(() => {
    const map: Record<string, TaskWithAttemptStatus> = {};
    for (const task of projectTasks) {
      map[task.id] = task;
    }
    return map;
  }, [projectTasks]);

  const epicOptions = useMemo(
    () => projectTasks.filter((task) => task.task_type === 'epic'),
    [projectTasks]
  );
  const featureOptions = useMemo(
    () => projectTasks.filter((task) => task.task_type === 'feature'),
    [projectTasks]
  );
  const storyOptions = useMemo(
    () => projectTasks.filter((task) => task.task_type === 'story'),
    [projectTasks]
  );

  // Get default form values based on mode
  const defaultValues = useMemo((): TaskFormValues => {
    const baseProfile = system.config?.executor_profile || null;

    switch (mode) {
      case 'edit':
        return {
          title: props.task.title,
          description: props.task.description || '',
          status: props.task.status,
          taskType: props.task.task_type,
          epicId: props.task.epic_id,
          parentTaskId: props.task.parent_task_id,
          storyPoints:
            props.task.story_points != null ? String(props.task.story_points) : '',
          executorProfileId: baseProfile,
          repoBranches: defaultRepoBranches,
          autoStart: false,
        };

      case 'duplicate':
        return {
          title: props.initialTask.title,
          description: props.initialTask.description || '',
          status: 'todo',
          taskType: props.initialTask.task_type,
          epicId: props.initialTask.epic_id,
          parentTaskId: props.initialTask.parent_task_id,
          storyPoints:
            props.initialTask.story_points != null
              ? String(props.initialTask.story_points)
              : '',
          executorProfileId: baseProfile,
          repoBranches: defaultRepoBranches,
          autoStart: true,
        };

      case 'subtask':
      case 'create':
      default:
        return {
          title: '',
          description: '',
          status: 'todo',
          taskType: 'task',
          epicId: null,
          parentTaskId: null,
          storyPoints: '',
          executorProfileId: baseProfile,
          repoBranches: defaultRepoBranches,
          autoStart: true,
        };
    }
  }, [mode, props, system.config?.executor_profile, defaultRepoBranches]);

  // Form submission handler
  const handleSubmit = async ({ value }: { value: TaskFormValues }) => {
    const storyPoints =
      value.storyPoints.trim() === '' ? null : Number(value.storyPoints);
    const normalizedStoryPoints =
      storyPoints == null || Number.isNaN(storyPoints) ? null : storyPoints;

    if (editMode) {
      await updateTask.mutateAsync(
        {
          taskId: props.task.id,
          data: {
            title: value.title,
            description: value.description,
            status: value.status,
            sprint_id: props.task.sprint_id,
            task_type: value.taskType,
            epic_id: value.epicId,
            parent_task_id: value.parentTaskId,
            story_points: normalizedStoryPoints,
            parent_workspace_id: props.task.parent_workspace_id,
            image_ids: images.length > 0 ? images.map((img) => img.id) : null,
          },
        },
        { onSuccess: () => modal.remove() }
      );
    } else {
      const imageIds =
        newlyUploadedImageIds.length > 0 ? newlyUploadedImageIds : null;
      const taskType: TaskType = mode === 'subtask' ? 'task' : value.taskType;
      const task = {
        project_id: projectId,
        title: value.title,
        description: value.description,
        status: null,
        sprint_id: null,
        task_type: taskType,
        epic_id: mode === 'subtask' ? null : value.epicId,
        parent_task_id: mode === 'subtask' ? null : value.parentTaskId,
        story_points: mode === 'subtask' ? null : normalizedStoryPoints,
        parent_workspace_id:
          mode === 'subtask' ? props.parentTaskAttemptId : null,
        image_ids: imageIds,
        shared_task_id: null,
      };
      const shouldAutoStart = value.autoStart && !forceCreateOnlyRef.current;
      if (shouldAutoStart) {
        const repos = value.repoBranches.map((rb) => ({
          repo_id: rb.repoId,
          target_branch: rb.branch,
        }));
        await createAndStart.mutateAsync(
          {
            task,
            executor_profile_id: value.executorProfileId!,
            repos,
          },
          { onSuccess: () => modal.remove() }
        );
      } else {
        await createTask.mutateAsync(task, { onSuccess: () => modal.remove() });
      }
    }
  };

  const validator = (value: TaskFormValues): string | undefined => {
    if (!value.title.trim().length) return 'need title';

    if (mode === 'subtask' && value.taskType !== 'task') {
      return 'subtasks must be type task';
    }

    if ((value.taskType === 'feature' || value.taskType === 'story') && !value.parentTaskId) {
      return 'need parent task';
    }

    if (value.storyPoints.trim().length > 0) {
      const points = Number(value.storyPoints);
      if (!Number.isInteger(points) || points < 0) {
        return 'story points must be a non-negative integer';
      }
      if (value.taskType === 'epic' || value.taskType === 'feature') {
        return 'story points are only valid for stories/tasks';
      }
    }

    if (value.autoStart && !forceCreateOnlyRef.current) {
      if (!value.executorProfileId) return 'need executor profile';
      if (
        value.repoBranches.length === 0 ||
        value.repoBranches.some((rb) => !rb.branch)
      ) {
        return 'need branch for all repos';
      }
    }
  };

  // Initialize TanStack Form
  const form = useForm({
    defaultValues: defaultValues,
    onSubmit: handleSubmit,
    validators: {
      // we use an onMount validator so that the primary action button can
      // enable/disable itself based on `canSubmit`
      onMount: ({ value }) => validator(value),
      onChange: ({ value }) => validator(value),
    },
  });

  const isSubmitting = useStore(form.store, (state) => state.isSubmitting);
  const isDirty = useStore(form.store, (state) => state.isDirty);
  const canSubmit = useStore(form.store, (state) => state.canSubmit);
  const currentTaskType = useStore(form.store, (state) => state.values.taskType);
  const currentParentTaskId = useStore(
    form.store,
    (state) => state.values.parentTaskId
  );

  // Load images for edit mode
  useEffect(() => {
    if (!taskImages) return;
    setImages(taskImages);
  }, [taskImages]);

  const onDrop = useCallback(
    async (files: File[]) => {
      for (const file of files) {
        try {
          // In edit mode, use uploadForTask to associate immediately
          // In create mode, use plain upload (will associate on task creation)
          const img = editMode
            ? await uploadForTask(props.task.id, file)
            : await upload(file);

          // Add markdown image reference to description
          const markdownText = `![${img.original_name}](${img.file_path})`;
          form.setFieldValue('description', (prev) =>
            prev.trim() === '' ? markdownText : `${prev} ${markdownText}`
          );
          setImages((prev) => [...prev, img]);
          setNewlyUploadedImageIds((prev) => [...prev, img.id]);
        } catch {
          // Silently ignore upload errors for now
        }
      }
    },
    [editMode, props, upload, uploadForTask, form]
  );

  const {
    getRootProps,
    getInputProps,
    isDragActive,
    open: dropzoneOpen,
  } = useDropzone({
    onDrop: onDrop,
    accept: { 'image/*': [] },
    disabled: isSubmitting,
    noClick: true,
    noKeyboard: true,
  });

  // Compute localImages for WYSIWYG rendering of uploaded images
  const localImages: LocalImageMetadata[] = useMemo(
    () =>
      images.map((img) => ({
        path: img.file_path,
        proxy_url: `/api/images/${img.id}/file`,
        file_name: img.original_name,
        size_bytes: Number(img.size_bytes),
        format: img.mime_type?.split('/')[1] ?? 'png',
      })),
    [images]
  );

  // Unsaved changes detection
  const hasUnsavedChanges = useCallback(() => {
    if (isDirty) return true;
    if (newlyUploadedImageIds.length > 0) return true;
    if (images.length > 0 && !editMode) return true;
    return false;
  }, [isDirty, newlyUploadedImageIds, images, editMode]);

  // beforeunload listener
  useEffect(() => {
    if (!modal.visible || isSubmitting) return;

    const handleBeforeUnload = (e: BeforeUnloadEvent) => {
      if (hasUnsavedChanges()) {
        e.preventDefault();
        return '';
      }
    };

    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => window.removeEventListener('beforeunload', handleBeforeUnload);
  }, [modal.visible, isSubmitting, hasUnsavedChanges]);

  // Keyboard shortcuts
  const primaryAction = useCallback(() => {
    if (isSubmitting || !canSubmit) return;
    void form.handleSubmit();
  }, [form, isSubmitting, canSubmit]);

  const shortcutsEnabled =
    modal.visible && !isSubmitting && canSubmit && !showDiscardWarning;

  useKeySubmitTask(primaryAction, {
    enabled: shortcutsEnabled,
    scope: Scope.DIALOG,
    enableOnFormTags: ['input', 'INPUT', 'textarea', 'TEXTAREA'],
    preventDefault: true,
  });

  const canSubmitAlt = useStore(
    form.store,
    (state) => state.values.title.trim().length > 0 && !state.isSubmitting
  );

  const handleSubmitCreateOnly = useCallback(() => {
    forceCreateOnlyRef.current = true;
    const promise = form.handleSubmit();
    Promise.resolve(promise).finally(() => {
      forceCreateOnlyRef.current = false;
    });
  }, [form]);

  useKeySubmitTaskAlt(handleSubmitCreateOnly, {
    enabled: modal.visible && canSubmitAlt && !showDiscardWarning,
    scope: Scope.DIALOG,
    enableOnFormTags: ['input', 'INPUT', 'textarea', 'TEXTAREA'],
    preventDefault: true,
  });

  // Dialog close handling
  const handleDialogClose = (open: boolean) => {
    if (open) return;
    if (hasUnsavedChanges()) {
      setShowDiscardWarning(true);
    } else {
      modal.remove();
    }
  };

  const handleDiscardChanges = () => {
    form.reset();
    setImages([]);
    setNewlyUploadedImageIds([]);
    setShowDiscardWarning(false);
    modal.remove();
  };

  const handleContinueEditing = () => {
    setShowDiscardWarning(false);
  };

  // Manage CONFIRMATION scope when warning is shown
  useEffect(() => {
    if (showDiscardWarning) {
      disableScope(Scope.DIALOG);
      enableScope(Scope.CONFIRMATION);
    } else {
      disableScope(Scope.CONFIRMATION);
      enableScope(Scope.DIALOG);
    }
  }, [showDiscardWarning, enableScope, disableScope]);

  useKeyExit(handleContinueEditing, {
    scope: Scope.CONFIRMATION,
    when: () => modal.visible && showDiscardWarning,
  });

  const loading = branchesLoading || userSystemLoading;
  if (loading) return <></>;

  return (
    <>
      <Dialog
        open={modal.visible}
        onOpenChange={handleDialogClose}
        className="w-full max-w-[min(90vw,40rem)] max-h-[min(95vh,50rem)] flex flex-col overflow-hidden"
        uncloseable={showDiscardWarning}
      >
        <div
          {...getRootProps()}
          className="h-full flex flex-col gap-4 p-4 relative min-h-0"
        >
          <input {...getInputProps()} />
          {/* Drag overlay */}
          {isDragActive && (
            <div className="absolute inset-0 z-50 bg-primary/95 border-2 border-dashed border-primary-foreground/50 rounded-lg flex items-center justify-center pointer-events-none">
              <div className="text-center">
                <ImageIcon className="h-12 w-12 mx-auto mb-2 text-primary-foreground" />
                <p className="text-lg font-medium text-primary-foreground">
                  {t('taskFormDialog.dropImagesHere')}
                </p>
              </div>
            </div>
          )}

          {/* Title */}
          <div className="flex-none px-4 py-2 border border-1 border-border">
            <form.Field name="title">
              {(field) => (
                <Input
                  id="task-title"
                  value={field.state.value}
                  onChange={(e) => field.handleChange(e.target.value)}
                  placeholder={t('taskFormDialog.titlePlaceholder')}
                  className="text-lg font-semibold placeholder:text-muted-foreground/60 border-none p-0"
                  disabled={isSubmitting}
                  autoFocus
                />
              )}
            </form.Field>
          </div>

          <div className="flex-1 p-4 min-h-0 overflow-y-auto overscroll-contain space-y-1 border border-1 border-border">
            {/* Description */}
            <form.Field name="description">
              {(field) => (
                <WYSIWYGEditor
                  placeholder={t('taskFormDialog.descriptionPlaceholder')}
                  value={field.state.value}
                  onChange={(desc) => field.handleChange(desc)}
                  disabled={isSubmitting}
                  projectId={projectId}
                  onPasteFiles={onDrop}
                  className="border-none shadow-none px-0 text-md font-normal"
                  onCmdEnter={primaryAction}
                  onShiftCmdEnter={handleSubmitCreateOnly}
                  taskId={editMode ? props.task.id : undefined}
                  localImages={localImages}
                />
              )}
            </form.Field>
            {/* Edit mode status */}
            {editMode && (
              <form.Field name="status">
                {(field) => (
                  <div className="space-y-2">
                    <Label
                      htmlFor="task-status"
                      className="text-sm font-medium"
                    >
                      {t('taskFormDialog.statusLabel')}
                    </Label>
                    <Select
                      value={field.state.value}
                      onValueChange={(value) =>
                        field.handleChange(value as TaskStatus)
                      }
                      disabled={isSubmitting}
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="todo">
                          {t('taskFormDialog.statusOptions.todo')}
                        </SelectItem>
                        <SelectItem value="inprogress">
                          {t('taskFormDialog.statusOptions.inprogress')}
                        </SelectItem>
                        <SelectItem value="inreview">
                          {t('taskFormDialog.statusOptions.inreview')}
                        </SelectItem>
                        <SelectItem value="done">
                          {t('taskFormDialog.statusOptions.done')}
                        </SelectItem>
                        <SelectItem value="cancelled">
                          {t('taskFormDialog.statusOptions.cancelled')}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                )}
              </form.Field>
            )}

            {!isSubtask ? (
              <div className="pt-4 mt-4 border-t space-y-3">
                <div className="grid gap-3 md:grid-cols-2">
                  <form.Field name="taskType">
                    {(field) => (
                      <div className="space-y-2">
                        <Label className="text-sm font-medium">Type</Label>
                        <Select
                          value={field.state.value}
                          onValueChange={(value) => {
                            const nextType = value as TaskType;
                            field.handleChange(nextType);
                            if (nextType === 'epic') {
                              form.setFieldValue('epicId', null);
                              form.setFieldValue('parentTaskId', null);
                              form.setFieldValue('storyPoints', '');
                            } else if (nextType === 'feature') {
                              form.setFieldValue('epicId', null);
                              form.setFieldValue('parentTaskId', null);
                              form.setFieldValue('storyPoints', '');
                            } else if (nextType === 'story') {
                              form.setFieldValue('epicId', null);
                              form.setFieldValue('parentTaskId', null);
                            }
                          }}
                          disabled={isSubmitting}
                        >
                          <SelectTrigger>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="task">Task</SelectItem>
                            <SelectItem value="story">User story</SelectItem>
                            <SelectItem value="feature">Feature</SelectItem>
                            <SelectItem value="epic">Epic</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                    )}
                  </form.Field>

                  <form.Field name="storyPoints">
                    {(field) => (
                      <div className="space-y-2">
                        <Label className="text-sm font-medium">
                          Story points
                        </Label>
                        <Input
                          type="number"
                          min={0}
                          step={1}
                          value={field.state.value}
                          onChange={(e) => field.handleChange(e.target.value)}
                          disabled={
                            isSubmitting ||
                            (currentTaskType !== 'story' &&
                              currentTaskType !== 'task')
                          }
                          placeholder={
                            currentTaskType === 'story' ||
                            currentTaskType === 'task'
                              ? 'e.g. 3'
                              : 'N/A'
                          }
                        />
                      </div>
                    )}
                  </form.Field>
                </div>

                {currentTaskType === 'feature' ? (
                  <form.Field name="parentTaskId">
                    {(field) => (
                      <div className="space-y-2">
                        <Label className="text-sm font-medium">Epic</Label>
                        <Select
                          value={field.state.value ?? 'none'}
                          onValueChange={(value) => {
                            const next = value === 'none' ? null : value;
                            field.handleChange(next);
                            form.setFieldValue('epicId', next);
                          }}
                          disabled={isSubmitting}
                        >
                          <SelectTrigger>
                            <SelectValue placeholder="Select an epic" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="none">None</SelectItem>
                            {epicOptions.map((epic) => (
                              <SelectItem key={epic.id} value={epic.id}>
                                {epic.title}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                    )}
                  </form.Field>
                ) : null}

                {currentTaskType === 'story' ? (
                  <form.Field name="parentTaskId">
                    {(field) => (
                      <div className="space-y-2">
                        <Label className="text-sm font-medium">Feature</Label>
                        <Select
                          value={field.state.value ?? 'none'}
                          onValueChange={(value) => {
                            const next = value === 'none' ? null : value;
                            field.handleChange(next);
                            if (next) {
                              form.setFieldValue(
                                'epicId',
                                projectTasksById[next]?.epic_id ?? null
                              );
                            } else {
                              form.setFieldValue('epicId', null);
                            }
                          }}
                          disabled={isSubmitting}
                        >
                          <SelectTrigger>
                            <SelectValue placeholder="Select a feature" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="none">None</SelectItem>
                            {featureOptions.map((feature) => (
                              <SelectItem key={feature.id} value={feature.id}>
                                {feature.title}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                    )}
                  </form.Field>
                ) : null}

                {currentTaskType === 'task' ? (
                  <div className="grid gap-3 md:grid-cols-2">
                    <form.Field name="parentTaskId">
                      {(field) => (
                        <div className="space-y-2">
                          <Label className="text-sm font-medium">
                            Parent (optional)
                          </Label>
                          <Select
                            value={field.state.value ?? 'none'}
                            onValueChange={(value) => {
                              const next = value === 'none' ? null : value;
                              field.handleChange(next);
                              if (next) {
                                const parent = projectTasksById[next];
                                form.setFieldValue('epicId', parent?.epic_id ?? null);
                              }
                            }}
                            disabled={isSubmitting}
                          >
                            <SelectTrigger>
                              <SelectValue placeholder="Select parent" />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="none">None</SelectItem>
                              {featureOptions.map((feature) => (
                                <SelectItem key={feature.id} value={feature.id}>
                                  Feature: {feature.title}
                                </SelectItem>
                              ))}
                              {storyOptions.map((story) => (
                                <SelectItem key={story.id} value={story.id}>
                                  Story: {story.title}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>
                      )}
                    </form.Field>

                    <form.Field name="epicId">
                      {(field) => (
                        <div className="space-y-2">
                          <Label className="text-sm font-medium">
                            Epic (optional)
                          </Label>
                          <Select
                            value={field.state.value ?? 'none'}
                            onValueChange={(value) => {
                              const next = value === 'none' ? null : value;
                              field.handleChange(next);
                            }}
                            disabled={isSubmitting || Boolean(currentParentTaskId)}
                          >
                            <SelectTrigger>
                              <SelectValue placeholder="Select epic" />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="none">None</SelectItem>
                              {epicOptions.map((epic) => (
                                <SelectItem key={epic.id} value={epic.id}>
                                  {epic.title}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>
                      )}
                    </form.Field>
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>

          {/* Create mode dropdowns */}
          {!editMode && (
            <form.Field name="autoStart" mode="array">
              {(autoStartField) => {
                const isSingleRepo = repoBranchConfigs.length === 1;
                return (
                  <div
                    className={cn(
                      'py-2 my-2 transition-opacity duration-200',
                      isSingleRepo ? '' : 'space-y-3',
                      autoStartField.state.value
                        ? 'opacity-100'
                        : 'opacity-0 pointer-events-none'
                    )}
                  >
                    <div className="flex items-center gap-2">
                      <form.Field name="executorProfileId">
                        {(field) => (
                          <ExecutorProfileSelector
                            profiles={profiles}
                            selectedProfile={field.state.value}
                            onProfileSelect={(profile) =>
                              field.handleChange(profile)
                            }
                            disabled={
                              isSubmitting || !autoStartField.state.value
                            }
                            showLabel={false}
                            className="flex items-center gap-2 flex-row flex-[2] min-w-0"
                            itemClassName="flex-1 min-w-0"
                          />
                        )}
                      </form.Field>
                      {isSingleRepo && (
                        <form.Field name="repoBranches">
                          {(field) => {
                            const config = repoBranchConfigs[0];
                            const selectedBranch =
                              field.state.value.find(
                                (v) => v.repoId === config.repoId
                              )?.branch ?? config.targetBranch;
                            return (
                              <div
                                className={cn(
                                  'flex-1 min-w-0',
                                  isSubmitting &&
                                    'opacity-50 pointer-events-none'
                                )}
                              >
                                <BranchSelector
                                  branches={config.branches}
                                  selectedBranch={selectedBranch}
                                  onBranchSelect={(branch) => {
                                    field.handleChange([
                                      { repoId: config.repoId, branch },
                                    ]);
                                  }}
                                  placeholder={
                                    branchesLoading
                                      ? t('createAttemptDialog.loadingBranches')
                                      : t('createAttemptDialog.selectBranch')
                                  }
                                />
                              </div>
                            );
                          }}
                        </form.Field>
                      )}
                    </div>
                    {!isSingleRepo && (
                      <form.Field name="repoBranches">
                        {(field) => {
                          const configs = repoBranchConfigs.map((config) => ({
                            ...config,
                            targetBranch:
                              field.state.value.find(
                                (v) => v.repoId === config.repoId
                              )?.branch ?? config.targetBranch,
                          }));
                          return (
                            <RepoBranchSelector
                              configs={configs}
                              onBranchChange={(repoId, branch) => {
                                const newValue = field.state.value.map((v) =>
                                  v.repoId === repoId ? { ...v, branch } : v
                                );
                                if (
                                  !newValue.find((v) => v.repoId === repoId)
                                ) {
                                  newValue.push({ repoId, branch });
                                }
                                field.handleChange(newValue);
                              }}
                              isLoading={branchesLoading}
                              showLabel={true}
                              className={cn(
                                isSubmitting && 'opacity-50 pointer-events-none'
                              )}
                            />
                          );
                        }}
                      </form.Field>
                    )}
                  </div>
                );
              }}
            </form.Field>
          )}

          {/* Actions */}
          <div className="flex items-center justify-between gap-3">
            {/* Attach Image*/}
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={dropzoneOpen}
                className="h-9 w-9 p-0 rounded-none"
                aria-label={t('taskFormDialog.attachImage')}
              >
                <ImageIcon className="h-4 w-4" />
              </Button>
            </div>

            {/* Autostart switch */}
            <div className="flex items-center gap-3">
              {!editMode && (
                <form.Field name="autoStart">
                  {(field) => (
                    <div className="flex items-center gap-2">
                      <Switch
                        id="autostart-switch"
                        checked={field.state.value}
                        onCheckedChange={(checked) =>
                          field.handleChange(checked)
                        }
                        disabled={isSubmitting}
                        className="data-[state=checked]:bg-gray-900 dark:data-[state=checked]:bg-gray-100"
                        aria-label={t('taskFormDialog.startLabel')}
                      />
                      <Label
                        htmlFor="autostart-switch"
                        className="text-sm cursor-pointer"
                      >
                        {t('taskFormDialog.startLabel')}
                      </Label>
                    </div>
                  )}
                </form.Field>
              )}

              {/* Create/Start/Update button*/}
              <form.Subscribe
                selector={(state) => ({
                  canSubmit: state.canSubmit,
                  isSubmitting: state.isSubmitting,
                  values: state.values,
                })}
              >
                {({ canSubmit, isSubmitting, values }) => {
                  const buttonText = editMode
                    ? isSubmitting
                      ? t('taskFormDialog.updating')
                      : t('taskFormDialog.updateTask')
                    : isSubmitting
                      ? values.autoStart
                        ? t('taskFormDialog.starting')
                        : t('taskFormDialog.creating')
                      : t('taskFormDialog.create');

                  return (
                    <Button onClick={form.handleSubmit} disabled={!canSubmit}>
                      {buttonText}
                    </Button>
                  );
                }}
              </form.Subscribe>
            </div>
          </div>
        </div>
      </Dialog>
      {showDiscardWarning && (
        <div className="fixed inset-0 z-[10000] flex items-start justify-center p-4 overflow-y-auto">
          <div
            className="fixed inset-0 bg-black/50"
            onClick={() => setShowDiscardWarning(false)}
          />
          <div className="relative z-[10000] grid w-full max-w-lg gap-4 bg-primary p-6 shadow-lg duration-200 sm:rounded-lg my-8">
            <DialogContent className="sm:max-w-[425px]">
              <DialogHeader>
                <div className="flex items-center gap-3">
                  <DialogTitle>
                    {t('taskFormDialog.discardDialog.title')}
                  </DialogTitle>
                </div>
                <DialogDescription className="text-left pt-2">
                  {t('taskFormDialog.discardDialog.description')}
                </DialogDescription>
              </DialogHeader>
              <DialogFooter className="gap-2">
                <Button variant="outline" onClick={handleContinueEditing}>
                  {t('taskFormDialog.discardDialog.continueEditing')}
                </Button>
                <Button variant="destructive" onClick={handleDiscardChanges}>
                  {t('taskFormDialog.discardDialog.discardChanges')}
                </Button>
              </DialogFooter>
            </DialogContent>
          </div>
        </div>
      )}
    </>
  );
});

export const TaskFormDialog = defineModal<TaskFormDialogProps, void>(
  TaskFormDialogImpl
);
