import { useDiffStream } from '@/hooks/useDiffStream';
import { useMemo, useCallback, useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader } from '@/components/ui/loader';
import { Button } from '@/components/ui/button';
import DiffViewSwitch from '@/components/DiffViewSwitch';
import DiffCard from '@/components/DiffCard';
import { NewCardHeader } from '@/components/ui/new-card';
import { ChevronsUp, ChevronsDown } from 'lucide-react';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import type { Diff, DiffChangeKind, UnifiedPrComment } from 'shared/types';
import type { Workspace } from 'shared/types';
import GitOperations, {
  type GitOperationsInputs,
} from '@/components/tasks/Toolbar/GitOperations.tsx';
import { useAttemptRepo } from '@/hooks/useAttemptRepo';
import { useRepoDiff } from '@/hooks/useRepoDiff';
import { usePrComments } from '@/hooks/usePrComments';
import { useReview } from '@/contexts/ReviewProvider';
import { SplitSide } from '@git-diff-view/react';
import { attemptsApi } from '@/lib/api';

interface DiffsPanelProps {
  selectedAttempt: Workspace | null;
  gitOps?: GitOperationsInputs;
}

type DiffSource = 'worktree' | 'branch';

type DiffCollapseDefaults = Record<DiffChangeKind, boolean>;

const DEFAULT_DIFF_COLLAPSE_DEFAULTS: DiffCollapseDefaults = {
  added: false,
  deleted: true,
  modified: false,
  renamed: true,
  copied: true,
  permissionChange: true,
};

const DEFAULT_COLLAPSE_MAX_LINES = 200;
const EMPTY_DIFFS: Diff[] = [];
const EMPTY_PR_COMMENTS: UnifiedPrComment[] = [];

const exceedsMaxLineCount = (d: Diff, maxLines: number): boolean => {
  if (d.additions != null || d.deletions != null)
    return (d.additions ?? 0) + (d.deletions ?? 0) > maxLines;

  return true;
};

const getDiffId = ({ diff, index }: { diff: Diff; index: number }) =>
  `${diff.newPath || diff.oldPath || index}`;

export function DiffsPanel({ selectedAttempt, gitOps }: DiffsPanelProps) {
  const { t } = useTranslation('tasks');
  const [worktreeLoadingState, setWorktreeLoadingState] = useState<
    'loading' | 'loaded' | 'timed-out'
  >('loading');
  const [diffSource, setDiffSource] = useState<DiffSource>('worktree');
  const [collapsedIds, setCollapsedIds] = useState<Set<string>>(new Set());
  const [processedIds, setProcessedIds] = useState<Set<string>>(new Set());
  const [syncing, setSyncing] = useState(false);
  const [syncError, setSyncError] = useState<string | null>(null);

  const attemptId = selectedAttempt?.id ?? null;
  const { selectedRepoId } = useAttemptRepo(selectedAttempt?.id ?? undefined);
  const { comments: localComments, clearComments } = useReview();

  const worktreeStreamEnabled = diffSource === 'worktree';
  const { diffs: worktreeDiffs, error: worktreeError } = useDiffStream(
    attemptId,
    worktreeStreamEnabled
  );

  const repoDiff = useRepoDiff(selectedAttempt?.id, selectedRepoId ?? undefined, {
    enabled: diffSource === 'branch',
  });

  const selectedRepoStatus = useMemo(() => {
    if (!gitOps?.branchStatus || !selectedRepoId) return null;
    return gitOps.branchStatus.find((r) => r.repo_id === selectedRepoId) ?? null;
  }, [gitOps?.branchStatus, selectedRepoId]);

  const hasAttachedPr = useMemo(() => {
    return (
      (selectedRepoStatus?.merges ?? []).some((m) => m.type === 'pr') ?? false
    );
  }, [selectedRepoStatus?.merges]);

  const prCommentsQuery = usePrComments(
    selectedAttempt?.id,
    selectedRepoId ?? undefined,
    {
      enabled: diffSource === 'branch' && hasAttachedPr,
    }
  );

  const prComments = prCommentsQuery.data?.comments ?? EMPTY_PR_COMMENTS;

  const diffs = useMemo(() => {
    if (diffSource === 'branch') return repoDiff.data ?? EMPTY_DIFFS;
    return worktreeDiffs;
  }, [diffSource, repoDiff.data, worktreeDiffs]);

  const error = useMemo(() => {
    if (diffSource === 'branch') {
      return (repoDiff.error as Error | null)?.message ?? null;
    }
    return worktreeError;
  }, [diffSource, repoDiff.error, worktreeError]);

  const { fileCount, added, deleted } = useMemo(() => {
    if (diffs.length === 0) return { fileCount: 0, added: 0, deleted: 0 };

    return diffs.reduce(
      (acc, d) => {
        acc.added += d.additions ?? 0;
        acc.deleted += d.deletions ?? 0;
        return acc;
      },
      { fileCount: diffs.length, added: 0, deleted: 0 }
    );
  }, [diffs]);

  // If no diffs arrive within 3 seconds, stop showing the spinner
  useEffect(() => {
    if (!worktreeStreamEnabled) return;
    if (worktreeLoadingState !== 'loading') return;
    const timer = setTimeout(
      () => setWorktreeLoadingState('timed-out'),
      3000
    );
    return () => clearTimeout(timer);
  }, [worktreeLoadingState, worktreeStreamEnabled]);

  useEffect(() => {
    if (!worktreeStreamEnabled) return;
    if (worktreeDiffs.length > 0 && worktreeLoadingState === 'loading') {
      setWorktreeLoadingState('loaded');
    }
  }, [worktreeDiffs.length, worktreeLoadingState, worktreeStreamEnabled]);

  useEffect(() => {
    setCollapsedIds(new Set());
    setProcessedIds(new Set());
    if (worktreeStreamEnabled) setWorktreeLoadingState('loading');
  }, [attemptId, selectedRepoId, diffSource, worktreeStreamEnabled]);

  useEffect(() => {
    if (diffs.length === 0) return;

    const newDiffs = diffs
      .map((d, index) => ({ diff: d, index }))
      .filter((d) => {
        const id = getDiffId(d);
        return !processedIds.has(id);
      });

    if (newDiffs.length === 0) return;

    const newIds = newDiffs.map(getDiffId);
    const toCollapse = newDiffs
      .filter(
        ({ diff }) =>
          DEFAULT_DIFF_COLLAPSE_DEFAULTS[diff.change] ||
          exceedsMaxLineCount(diff, DEFAULT_COLLAPSE_MAX_LINES)
      )
      .map(getDiffId);

    setProcessedIds((prev) => new Set([...prev, ...newIds]));
    if (toCollapse.length > 0) {
      setCollapsedIds((prev) => new Set([...prev, ...toCollapse]));
    }
  }, [diffs, processedIds]);

  const loading =
    diffSource === 'branch'
      ? repoDiff.isLoading
      : worktreeLoadingState === 'loading';

  const canSyncToPr =
    diffSource === 'branch' &&
    !!selectedAttempt?.id &&
    !!selectedRepoId &&
    hasAttachedPr &&
    localComments.length > 0;

  const handleSyncToPr = useCallback(async () => {
    if (!selectedAttempt?.id || !selectedRepoId) return;
    if (localComments.length === 0) return;
    setSyncError(null);
    setSyncing(true);
    try {
      await attemptsApi.submitPrReviewComments(selectedAttempt.id, {
        repo_id: selectedRepoId,
        comments: localComments.map((c) => ({
          path: c.filePath,
          line: c.lineNumber,
          side: c.side === SplitSide.old ? 'LEFT' : 'RIGHT',
          body: c.text,
        })),
      });
      clearComments();
      await prCommentsQuery.refetch();
    } catch (e) {
      const msg =
        e instanceof Error ? e.message : 'Failed to sync PR review comments';
      setSyncError(msg);
    } finally {
      setSyncing(false);
    }
  }, [
    clearComments,
    localComments,
    prCommentsQuery,
    selectedAttempt?.id,
    selectedRepoId,
  ]);

  const ids = useMemo(() => {
    return diffs.map((d, i) => getDiffId({ diff: d, index: i }));
  }, [diffs]);

  const toggle = useCallback((id: string) => {
    setCollapsedIds((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }, []);

  const allCollapsed = collapsedIds.size === diffs.length;
  const handleCollapseAll = useCallback(() => {
    setCollapsedIds(allCollapsed ? new Set() : new Set(ids));
  }, [allCollapsed, ids]);

  if (error) {
    return (
      <div className="bg-red-50 border border-red-200 rounded-lg p-4 m-4">
        <div className="text-red-800 text-sm">
          {t('diff.errorLoadingDiff', { error })}
        </div>
      </div>
    );
  }

  return (
    <DiffsPanelContent
      diffs={diffs}
      fileCount={fileCount}
      added={added}
      deleted={deleted}
      collapsedIds={collapsedIds}
      allCollapsed={allCollapsed}
      handleCollapseAll={handleCollapseAll}
      toggle={toggle}
      selectedAttempt={selectedAttempt}
      gitOps={gitOps}
      loading={loading}
      diffSource={diffSource}
      setDiffSource={setDiffSource}
      prComments={prComments}
      syncError={syncError}
      canSyncToPr={canSyncToPr}
      syncing={syncing}
      onSyncToPr={handleSyncToPr}
      t={t}
    />
  );
}

interface DiffsPanelContentProps {
  diffs: Diff[];
  fileCount: number;
  added: number;
  deleted: number;
  collapsedIds: Set<string>;
  allCollapsed: boolean;
  handleCollapseAll: () => void;
  toggle: (id: string) => void;
  selectedAttempt: Workspace | null;
  gitOps?: GitOperationsInputs;
  loading: boolean;
  diffSource: DiffSource;
  setDiffSource: (source: DiffSource) => void;
  prComments: UnifiedPrComment[];
  syncError: string | null;
  canSyncToPr: boolean;
  syncing: boolean;
  onSyncToPr: () => void;
  t: (key: string, params?: Record<string, unknown>) => string;
}

function DiffsPanelContent({
  diffs,
  fileCount,
  added,
  deleted,
  collapsedIds,
  allCollapsed,
  handleCollapseAll,
  toggle,
  selectedAttempt,
  gitOps,
  loading,
  diffSource,
  setDiffSource,
  prComments,
  syncError,
  canSyncToPr,
  syncing,
  onSyncToPr,
  t,
}: DiffsPanelContentProps) {
  return (
    <div className="h-full flex flex-col relative">
      {selectedAttempt && (
        <NewCardHeader
          className="sticky top-0 z-10"
          actions={
            <>
              {gitOps && selectedAttempt && (
                <>
                  <div className="flex items-center gap-1">
                    <Button
                      size="sm"
                      variant={
                        diffSource === 'worktree' ? 'secondary' : 'outline'
                      }
                      onClick={() => setDiffSource('worktree')}
                      aria-pressed={diffSource === 'worktree'}
                    >
                      Worktree
                    </Button>
                    <Button
                      size="sm"
                      variant={diffSource === 'branch' ? 'secondary' : 'outline'}
                      onClick={() => setDiffSource('branch')}
                      aria-pressed={diffSource === 'branch'}
                    >
                      Branch
                    </Button>
                  </div>
                  <div className="h-4 w-px bg-border" />
                </>
              )}
              {diffSource === 'branch' && (
                <>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={onSyncToPr}
                    disabled={!canSyncToPr || syncing}
                    title={
                      canSyncToPr
                        ? 'Submit review comments to PR'
                        : 'Add review comments to enable syncing'
                    }
                  >
                    {syncing ? 'Syncing…' : 'Sync to PR'}
                  </Button>
                  <div className="h-4 w-px bg-border" />
                </>
              )}
              <DiffViewSwitch />
              <div className="h-4 w-px bg-border" />
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      variant="icon"
                      onClick={handleCollapseAll}
                      disabled={diffs.length === 0}
                      aria-pressed={allCollapsed}
                      aria-label={
                        allCollapsed
                          ? t('diff.expandAll')
                          : t('diff.collapseAll')
                      }
                    >
                      {allCollapsed ? (
                        <ChevronsDown className="h-4 w-4" />
                      ) : (
                        <ChevronsUp className="h-4 w-4" />
                      )}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    {allCollapsed ? t('diff.expandAll') : t('diff.collapseAll')}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </>
          }
        >
          <div className="flex items-center">
            <span
              className="text-sm text-muted-foreground whitespace-nowrap"
              aria-live="polite"
            >
              {t('diff.filesChanged', { count: fileCount })}{' '}
              <span className="text-green-600 dark:text-green-500">
                +{added}
              </span>{' '}
              <span className="text-red-600 dark:text-red-500">-{deleted}</span>
            </span>
          </div>
        </NewCardHeader>
      )}
      {gitOps && selectedAttempt && (
        <div className="px-3">
          <GitOperations selectedAttempt={selectedAttempt} {...gitOps} />
        </div>
      )}
      {syncError && (
        <div className="mx-3 mt-3 bg-red-50 border border-red-200 rounded-lg p-3 text-sm text-red-800">
          {syncError}
        </div>
      )}
      <div className="flex-1 overflow-y-auto px-3">
        {loading ? (
          <div className="flex items-center justify-center h-full">
            <Loader />
          </div>
        ) : diffs.length === 0 ? (
          <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
            {t('diff.noChanges')}
          </div>
        ) : (
          diffs.map((diff, idx) => {
            const id = diff.newPath || diff.oldPath || String(idx);
            return (
              <DiffCard
                key={id}
                diff={diff}
                expanded={!collapsedIds.has(id)}
                onToggle={() => toggle(id)}
                selectedAttempt={selectedAttempt}
                prComments={prComments}
              />
            );
          })
        )}
      </div>
    </div>
  );
}
