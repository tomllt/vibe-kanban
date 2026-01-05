import { useCallback, useMemo } from 'react';
import { useJsonPatchWsStream } from './useJsonPatchWsStream';
import type { Merge } from 'shared/types';

type MergesState = {
  merges: Record<string, Merge>;
};

export function useRepoMerges(workspaceId?: string, repoId?: string) {
  const enabled = Boolean(workspaceId && repoId);

  const endpoint = useMemo(() => {
    if (!workspaceId || !repoId) return undefined;
    const params = new URLSearchParams({
      workspace_id: workspaceId,
      repo_id: repoId,
    });
    return `/api/merges/stream/ws?${params.toString()}`;
  }, [workspaceId, repoId]);

  const initialData = useCallback((): MergesState => ({ merges: {} }), []);

  const { data, isConnected, error } = useJsonPatchWsStream(
    endpoint,
    enabled,
    initialData
  );
  const hasSnapshot = Boolean(data);

  const mergesById = useMemo(() => data?.merges ?? {}, [data?.merges]);

  const merges = useMemo(() => {
    const list = Object.values(mergesById);
    const getCreatedAt = (m: Merge) =>
      new Date((m as unknown as { created_at: string }).created_at).getTime();
    list.sort(
      (a, b) => getCreatedAt(b) - getCreatedAt(a)
    );
    return list;
  }, [mergesById]);

  return { merges, mergesById, isConnected, hasSnapshot, error };
}
