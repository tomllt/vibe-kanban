import { useEffect, useMemo, useState } from 'react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { useTranslation } from 'react-i18next';
import { Sparkles, Trash2, Plus, RefreshCcw } from 'lucide-react';

import { defineModal } from '@/lib/modals';
import { tasksApi } from '@/lib/api';

import type { BacklogGroomingDraft, TaskWithAttemptStatus } from 'shared/types';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Alert } from '@/components/ui/alert';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

export interface BacklogGroomerDialogProps {
  task: TaskWithAttemptStatus;
}

const storyPointOptions = ['1', '2', '3', '5', '8', '13'] as const;

const clampList = (items: string[], max: number) => items.slice(0, max);

const normalizeList = (items: string[]) =>
  items.map((s) => s.trim()).filter(Boolean);

const BacklogGroomerDialogImpl = NiceModal.create<BacklogGroomerDialogProps>(
  ({ task }) => {
    const modal = useModal();
    const { t } = useTranslation('tasks');

    const [draft, setDraft] = useState<BacklogGroomingDraft | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [isApplying, setIsApplying] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const canApply = useMemo(() => {
      if (!draft) return false;
      const ac = normalizeList(draft.acceptance_criteria);
      const subtasks = normalizeList(draft.subtasks);
      const hasValidCounts = ac.length >= 1 && subtasks.length >= 3;
      const hasValidPoints = storyPointOptions.includes(
        String(draft.story_points) as (typeof storyPointOptions)[number]
      );
      return hasValidCounts && hasValidPoints;
    }, [draft]);

    useEffect(() => {
      let cancelled = false;
      (async () => {
        try {
          const existing = await tasksApi.getBacklogGroomingDraft(task.id);
          if (cancelled) return;
          setDraft(existing?.draft ?? null);
        } catch {
          // best-effort
        }
      })();
      return () => {
        cancelled = true;
      };
    }, [task.id]);

    const handleGenerate = async () => {
      setIsLoading(true);
      setError(null);
      try {
        const res = await tasksApi.generateBacklogGrooming(task.id);
        setDraft(res.draft);
      } catch (err: unknown) {
        const message =
          err instanceof Error ? err.message : t('backlogGroomer.errorGeneric');
        setError(message);
      } finally {
        setIsLoading(false);
      }
    };

    const handleApply = async () => {
      if (!draft) return;

      const acceptance_criteria = clampList(
        normalizeList(draft.acceptance_criteria),
        10
      );
      const subtasks = clampList(normalizeList(draft.subtasks), 5);

      if (subtasks.length < 3) {
        setError(t('backlogGroomer.validationSubtasks'));
        return;
      }

      setIsApplying(true);
      setError(null);
      try {
        const updated = await tasksApi.applyBacklogGrooming(task.id, {
          draft: {
            acceptance_criteria,
            subtasks,
            story_points: draft.story_points,
          },
        });
        modal.resolve(updated);
        modal.hide();
      } catch (err: unknown) {
        const message =
          err instanceof Error ? err.message : t('backlogGroomer.errorGeneric');
        setError(message);
      } finally {
        setIsApplying(false);
      }
    };

    const updateDraftItem = (
      field: 'acceptance_criteria' | 'subtasks',
      index: number,
      value: string
    ) => {
      setDraft((prev) => {
        if (!prev) return prev;
        const next = { ...prev };
        next[field] = [...next[field]];
        next[field][index] = value;
        return next;
      });
    };

    const removeDraftItem = (field: 'acceptance_criteria' | 'subtasks', i: number) =>
      setDraft((prev) => {
        if (!prev) return prev;
        const next = { ...prev };
        next[field] = prev[field].filter((_, idx) => idx !== i);
        return next;
      });

    const addDraftItem = (field: 'acceptance_criteria' | 'subtasks') =>
      setDraft((prev) => {
        if (!prev) return prev;
        const next = { ...prev };
        next[field] = [...prev[field], ''];
        return next;
      });

    const close = () => {
      modal.reject();
      modal.hide();
    };

    return (
      <Dialog open={modal.visible} onOpenChange={(open) => !open && close()}>
        <DialogContent className="max-w-3xl overflow-hidden">
          <div className="absolute inset-x-0 top-0 h-[2px] bg-gradient-to-r from-transparent via-foreground/40 to-transparent" />
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <span className="inline-flex h-8 w-8 items-center justify-center rounded-md border bg-muted">
                <Sparkles className="h-4 w-4" />
              </span>
              {t('backlogGroomer.title')}
            </DialogTitle>
            <DialogDescription className="text-left">
              <span className="font-medium">{task.title}</span>
              <span className="text-muted-foreground">
                {' '}
                · {t('backlogGroomer.description')}
              </span>
            </DialogDescription>
          </DialogHeader>

          {error && (
            <Alert variant="destructive" className="mb-4">
              {error}
            </Alert>
          )}

          {!draft ? (
            <div className="rounded-lg border bg-muted/40 p-6">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <div className="text-sm font-medium">
                    {t('backlogGroomer.emptyTitle')}
                  </div>
                  <div className="mt-1 text-sm text-muted-foreground">
                    {t('backlogGroomer.emptyBody')}
                  </div>
                </div>
                <Button onClick={handleGenerate} disabled={isLoading} autoFocus>
                  {isLoading
                    ? t('backlogGroomer.generating')
                    : t('backlogGroomer.generate')}
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-6">
              <div className="flex items-center justify-between gap-3">
                <div className="text-sm text-muted-foreground">
                  {t('backlogGroomer.generatedHint')}
                </div>
                <Button
                  variant="outline"
                  onClick={handleGenerate}
                  disabled={isLoading || isApplying}
                >
                  <RefreshCcw className="mr-2 h-4 w-4" />
                  {isLoading
                    ? t('backlogGroomer.generating')
                    : t('backlogGroomer.regenerate')}
                </Button>
              </div>

              <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
                <div className="space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="text-sm font-medium">
                      {t('backlogGroomer.storyPoints')}
                    </div>
                  </div>
                  <Select
                    value={String(draft.story_points)}
                    onValueChange={(v) =>
                      setDraft((prev) =>
                        prev ? { ...prev, story_points: Number(v) } : prev
                      )
                    }
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {storyPointOptions.map((v) => (
                        <SelectItem key={v} value={v}>
                          {v}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
                <div className="space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="text-sm font-medium">
                      {t('backlogGroomer.acceptanceCriteria')}
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => addDraftItem('acceptance_criteria')}
                      disabled={draft.acceptance_criteria.length >= 10}
                    >
                      <Plus className="mr-2 h-4 w-4" />
                      {t('backlogGroomer.add')}
                    </Button>
                  </div>
                  <div className="space-y-2">
                    {draft.acceptance_criteria.map((item, i) => (
                      <div key={i} className="flex items-start gap-2">
                        <Input
                          value={item}
                          onChange={(e) =>
                            updateDraftItem(
                              'acceptance_criteria',
                              i,
                              e.target.value
                            )
                          }
                          placeholder={t('backlogGroomer.itemPlaceholder')}
                        />
                        <Button
                          variant="icon"
                          onClick={() => removeDraftItem('acceptance_criteria', i)}
                          aria-label={t('backlogGroomer.remove')}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="text-sm font-medium">
                      {t('backlogGroomer.subtasks')}
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => addDraftItem('subtasks')}
                      disabled={draft.subtasks.length >= 5}
                    >
                      <Plus className="mr-2 h-4 w-4" />
                      {t('backlogGroomer.add')}
                    </Button>
                  </div>
                  <div className="space-y-2">
                    {draft.subtasks.map((item, i) => (
                      <div key={i} className="flex items-start gap-2">
                        <Input
                          value={item}
                          onChange={(e) =>
                            updateDraftItem('subtasks', i, e.target.value)
                          }
                          placeholder={t('backlogGroomer.itemPlaceholder')}
                        />
                        <Button
                          variant="icon"
                          onClick={() => removeDraftItem('subtasks', i)}
                          aria-label={t('backlogGroomer.remove')}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    ))}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t('backlogGroomer.subtasksHint')}
                  </div>
                </div>
              </div>
            </div>
          )}

          <DialogFooter>
            <Button
              variant="outline"
              onClick={close}
              disabled={isLoading || isApplying}
              autoFocus={!draft}
            >
              {t('common:buttons.cancel')}
            </Button>
            <Button
              onClick={handleApply}
              disabled={!draft || !canApply || isApplying || isLoading}
            >
              {isApplying ? t('backlogGroomer.applying') : t('backlogGroomer.apply')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
);

export const BacklogGroomerDialog = defineModal<
  BacklogGroomerDialogProps,
  unknown
>(BacklogGroomerDialogImpl);

