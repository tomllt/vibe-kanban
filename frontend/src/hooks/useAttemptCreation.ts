import { useMutation, useQueryClient } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';
import type {
  ExecutorProfileId,
  GitBranchKind,
  WorkspaceRepoInput,
  Workspace,
} from 'shared/types';

type CreateAttemptArgs = {
  profile: ExecutorProfileId;
  repos: WorkspaceRepoInput[];
  branchKind?: GitBranchKind;
};

type UseAttemptCreationArgs = {
  taskId: string;
  onSuccess?: (attempt: Workspace) => void;
};

export function useAttemptCreation({
  taskId,
  onSuccess,
}: UseAttemptCreationArgs) {
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: ({ profile, repos, branchKind }: CreateAttemptArgs) =>
      attemptsApi.create({
        task_id: taskId,
        executor_profile_id: profile,
        repos,
        ...(branchKind ? { branch_kind: branchKind } : {}),
      }),
    onSuccess: (newAttempt: Workspace) => {
      queryClient.setQueryData(
        ['taskAttempts', taskId],
        (old: Workspace[] = []) => [newAttempt, ...old]
      );
      onSuccess?.(newAttempt);
    },
  });

  return {
    createAttempt: mutation.mutateAsync,
    isCreating: mutation.isPending,
    error: mutation.error,
  };
}
