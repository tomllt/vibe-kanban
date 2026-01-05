import { useCallback, useState } from 'react';
import { attemptsApi } from '@/lib/api';
import type { ResolveConflictsError } from 'shared/types';

type ResolveState = {
  processId: string | null;
  startedAt: string | null;
};

function resolveErrorMessage(
  error: ResolveConflictsError | undefined,
  fallback: string | undefined
): string {
  switch (error?.type) {
    case 'no_conflicts':
      return 'No conflicted files were detected for this repo.';
    case 'process_already_running':
      return 'Another task process is running. Stop it before resolving conflicts.';
    case 'missing_executor_profile':
      return 'No prior coding agent run found for this attempt.';
    default:
      return fallback ?? 'Failed to start conflict resolution.';
  }
}

export function useResolveConflicts(attemptId?: string) {
  const [isResolving, setIsResolving] = useState(false);
  const [resolveError, setResolveError] = useState<string | null>(null);
  const [resolveState, setResolveState] = useState<ResolveState>({
    processId: null,
    startedAt: null,
  });

  const resolveConflicts = useCallback(
    async (repoId?: string) => {
      if (!attemptId || !repoId) return null;
      setIsResolving(true);
      setResolveError(null);
      try {
        const result = await attemptsApi.resolveConflicts(attemptId, {
          repo_id: repoId,
        });
        if (!result.success) {
          setResolveError(resolveErrorMessage(result.error, result.message));
          return null;
        }
        setResolveState({
          processId: result.data.id,
          startedAt: result.data.created_at,
        });
        return result.data;
      } catch (error) {
        const err = error as { message?: string };
        setResolveError(err.message ?? 'Failed to start conflict resolution.');
        return null;
      } finally {
        setIsResolving(false);
      }
    },
    [attemptId]
  );

  return {
    resolveConflicts,
    isResolving,
    resolveError,
    setResolveError,
    resolveProcessId: resolveState.processId,
    resolveStartedAt: resolveState.startedAt,
  } as const;
}
