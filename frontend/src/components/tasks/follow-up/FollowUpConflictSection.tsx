import { useEffect, useRef, useState } from 'react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { CheckCircle2, AlertCircle, Loader2 } from 'lucide-react';
import { ConflictBanner } from '@/components/tasks/ConflictBanner';
import { useOpenInEditor } from '@/hooks/useOpenInEditor';
import { useAttemptConflicts } from '@/hooks/useAttemptConflicts';
import type { RepoBranchStatus } from 'shared/types';

type Props = {
  workspaceId?: string;
  attemptBranch: string | null;
  branchStatus: RepoBranchStatus[] | undefined;
  isEditable: boolean;
  onResolve?: () => void;
  enableResolve: boolean;
  enableAbort: boolean;
  resolveStatus?: {
    tone: 'default' | 'success' | 'destructive';
    message: string;
  } | null;
  resolveError?: string | null;
  conflictResolutionInstructions: string | null;
};

export function FollowUpConflictSection({
  workspaceId,
  attemptBranch,
  branchStatus,
  onResolve,
  enableResolve,
  enableAbort,
  resolveStatus,
  resolveError,
  conflictResolutionInstructions,
}: Props) {
  const repoWithConflicts = branchStatus?.find(
    (r) => r.is_rebase_in_progress || (r.conflicted_files?.length ?? 0) > 0
  );
  const op = repoWithConflicts?.conflict_op ?? null;
  const openInEditor = useOpenInEditor(workspaceId);
  const repoId = repoWithConflicts?.repo_id;
  const { abortConflicts } = useAttemptConflicts(workspaceId, repoId);

  // write using setAborting and read through abortingRef in async handlers
  const [aborting, setAborting] = useState(false);
  const abortingRef = useRef(false);
  useEffect(() => {
    abortingRef.current = aborting;
  }, [aborting]);

  if (!repoWithConflicts && !resolveStatus && !resolveError) return null;

  return (
    <>
      {repoWithConflicts && (
        <ConflictBanner
          attemptBranch={attemptBranch}
          baseBranch={repoWithConflicts.target_branch_name ?? ''}
          conflictedFiles={repoWithConflicts.conflicted_files || []}
          op={op}
          onResolve={onResolve}
          enableResolve={enableResolve && !aborting}
          onOpenEditor={() => {
            if (!workspaceId) return;
            const first = repoWithConflicts.conflicted_files?.[0];
            openInEditor(first ? { filePath: first } : undefined);
          }}
          onAbort={async () => {
            if (!workspaceId) return;
            if (!enableAbort || abortingRef.current) return;
            try {
              setAborting(true);
              await abortConflicts();
            } catch (e) {
              console.error('Failed to abort conflicts', e);
            } finally {
              setAborting(false);
            }
          }}
          enableAbort={enableAbort && !aborting}
        />
      )}
      {resolveStatus && (
        <Alert variant={resolveStatus.tone} className="mb-4">
          {resolveStatus.tone === 'success' ? (
            <CheckCircle2 className="h-4 w-4" />
          ) : resolveStatus.tone === 'destructive' ? (
            <AlertCircle className="h-4 w-4" />
          ) : (
            <Loader2 className="h-4 w-4 animate-spin" />
          )}
          <AlertDescription>{resolveStatus.message}</AlertDescription>
        </Alert>
      )}
      {!resolveStatus && resolveError && (
        <Alert variant="destructive" className="mb-4">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>{resolveError}</AlertDescription>
        </Alert>
      )}
      {/* Conflict instructions preview (non-editable) */}
      {conflictResolutionInstructions && enableResolve && (
        <div className="text-sm mb-4">
          <div className="text-xs font-medium text-warning-foreground dark:text-warning mb-1">
            Conflict resolution instructions
          </div>
          <div className="whitespace-pre-wrap">
            {conflictResolutionInstructions}
          </div>
        </div>
      )}
    </>
  );
}
