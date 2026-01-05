import { useQuery } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';
import type { Diff } from 'shared/types';

export const repoDiffKeys = {
  all: ['repoDiff'] as const,
  byAttemptAndRepo: (
    attemptId: string | undefined,
    repoId: string | undefined,
    baseRef?: string,
    headRef?: string
  ) => ['repoDiff', attemptId, repoId, baseRef ?? null, headRef ?? null] as const,
};

type Options = {
  enabled?: boolean;
  baseRef?: string;
  headRef?: string;
};

export function useRepoDiff(attemptId?: string, repoId?: string, opts?: Options) {
  const enabled = (opts?.enabled ?? true) && !!attemptId && !!repoId;

  return useQuery<Diff[]>({
    queryKey: repoDiffKeys.byAttemptAndRepo(
      attemptId,
      repoId,
      opts?.baseRef,
      opts?.headRef
    ),
    queryFn: () =>
      attemptsApi.getRepoDiff(attemptId!, repoId!, {
        base_ref: opts?.baseRef,
        head_ref: opts?.headRef,
      }),
    enabled,
    staleTime: 10_000,
    retry: 1,
  });
}

