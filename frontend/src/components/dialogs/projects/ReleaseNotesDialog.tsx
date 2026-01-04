import { useEffect, useMemo, useState } from 'react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';

import type { ReleaseNotesResponse, Sprint } from 'shared/types';
import { sprintsApi } from '@/lib/api';
import { defineModal } from '@/lib/modals';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Alert, AlertDescription } from '@/components/ui/alert';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

export interface SprintReleaseNotesDialogProps {
  projectId: string;
  projectName: string;
}

export type SprintReleaseNotesDialogResult = 'closed';

function toDateInputValue(date: Date): string {
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, '0');
  const day = String(date.getUTCDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function isoAtStartOfDayUtc(dateStr: string): string {
  return new Date(`${dateStr}T00:00:00.000Z`).toISOString();
}

function isoAtStartOfNextDayUtc(dateStr: string): string {
  const dt = new Date(`${dateStr}T00:00:00.000Z`);
  dt.setUTCDate(dt.getUTCDate() + 1);
  return dt.toISOString();
}

function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

const SprintReleaseNotesDialogImpl = NiceModal.create<SprintReleaseNotesDialogProps>(
  ({ projectId, projectName }) => {
    const modal = useModal();

    const [sprints, setSprints] = useState<Sprint[]>([]);
    const [selectedSprintId, setSelectedSprintId] = useState<string>('');
    const [loadingSprints, setLoadingSprints] = useState(false);
    const [generating, setGenerating] = useState(false);

    const [createName, setCreateName] = useState('');
    const [createStartDate, setCreateStartDate] = useState('');
    const [createEndDate, setCreateEndDate] = useState('');
    const [creating, setCreating] = useState(false);

    const [releaseNotes, setReleaseNotes] = useState<ReleaseNotesResponse | null>(
      null
    );
    const [error, setError] = useState<string | null>(null);

    const selectedSprint = useMemo(
      () => sprints.find((s) => s.id === selectedSprintId) ?? null,
      [sprints, selectedSprintId]
    );

    useEffect(() => {
      if (!modal.visible) return;

      const now = new Date();
      const start = new Date(now);
      start.setUTCDate(start.getUTCDate() - 13);

      setCreateName(`Sprint ${toDateInputValue(now)}`);
      setCreateStartDate(toDateInputValue(start));
      setCreateEndDate(toDateInputValue(now));

      setError(null);
      setReleaseNotes(null);

      setLoadingSprints(true);
      sprintsApi
        .list(projectId)
        .then((data) => {
          setSprints(data);
          setSelectedSprintId(data[0]?.id ?? '');
        })
        .catch((e) => {
          setError(e instanceof Error ? e.message : 'Failed to load sprints');
        })
        .finally(() => setLoadingSprints(false));
    }, [modal.visible, projectId]);

    const handleClose = () => {
      modal.resolve('closed' as SprintReleaseNotesDialogResult);
      modal.hide();
    };

    const handleCreateSprint = async () => {
      setError(null);
      setReleaseNotes(null);

      if (!createName.trim()) {
        setError('Sprint name is required');
        return;
      }
      if (!createStartDate || !createEndDate) {
        setError('Start and end dates are required');
        return;
      }

      setCreating(true);
      try {
        const sprint = await sprintsApi.create(projectId, {
          name: createName.trim(),
          start_at: isoAtStartOfDayUtc(createStartDate),
          end_at: isoAtStartOfNextDayUtc(createEndDate),
        });
        const updated = await sprintsApi.list(projectId);
        setSprints(updated);
        setSelectedSprintId(sprint.id);
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Failed to create sprint');
      } finally {
        setCreating(false);
      }
    };

    const handleGenerate = async () => {
      if (!selectedSprintId) return;
      setError(null);
      setGenerating(true);
      try {
        const rn = await sprintsApi.getReleaseNotes(projectId, selectedSprintId);
        setReleaseNotes(rn);
      } catch (e) {
        setError(
          e instanceof Error ? e.message : 'Failed to generate release notes'
        );
      } finally {
        setGenerating(false);
      }
    };

    const handleCopy = async () => {
      if (!releaseNotes?.markdown) return;
      try {
        await navigator.clipboard.writeText(releaseNotes.markdown);
      } catch (e) {
        setError(
          e instanceof Error ? e.message : 'Failed to copy to clipboard'
        );
      }
    };

    const handleDownload = async () => {
      if (!selectedSprintId) return;
      setError(null);
      try {
        const blob = await sprintsApi.downloadReleaseNotes(
          projectId,
          selectedSprintId
        );
        const base =
          selectedSprint?.name?.trim().toLowerCase().replace(/\s+/g, '-') ||
          'sprint';
        downloadBlob(blob, `release-notes-${base}.md`);
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Failed to download');
      }
    };

    return (
      <Dialog open={modal.visible} onOpenChange={(open) => !open && handleClose()}>
        <DialogContent className="sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>Release notes</DialogTitle>
            <DialogDescription>
              Generate sprint-based release notes for <b>{projectName}</b>.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-6">
            {error && (
              <Alert variant="destructive">
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}

            <div className="space-y-2">
              <Label>Sprint</Label>
              <div className="flex gap-2">
                <div className="flex-1">
                  <Select
                    value={selectedSprintId}
                    onValueChange={setSelectedSprintId}
                    disabled={loadingSprints || sprints.length === 0}
                  >
                    <SelectTrigger>
                      <SelectValue
                        placeholder={
                          loadingSprints ? 'Loading…' : 'Select a sprint'
                        }
                      />
                    </SelectTrigger>
                    <SelectContent>
                      {sprints.map((s) => (
                        <SelectItem key={s.id} value={s.id}>
                          {s.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <Button
                  onClick={handleGenerate}
                  disabled={!selectedSprintId || generating}
                >
                  {generating ? 'Generating…' : 'Generate'}
                </Button>
              </div>
            </div>

            <div className="space-y-3 rounded-md border p-4">
              <div className="text-sm font-medium">Create sprint</div>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="space-y-1">
                  <Label htmlFor="sprint-name">Name</Label>
                  <Input
                    id="sprint-name"
                    value={createName}
                    onChange={(e) => setCreateName(e.target.value)}
                    placeholder="Sprint 42"
                    disabled={creating}
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="sprint-start">Start (UTC)</Label>
                  <Input
                    id="sprint-start"
                    type="date"
                    value={createStartDate}
                    onChange={(e) => setCreateStartDate(e.target.value)}
                    disabled={creating}
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="sprint-end">End (inclusive, UTC)</Label>
                  <Input
                    id="sprint-end"
                    type="date"
                    value={createEndDate}
                    onChange={(e) => setCreateEndDate(e.target.value)}
                    disabled={creating}
                  />
                </div>
              </div>
              <div className="flex justify-end">
                <Button onClick={handleCreateSprint} disabled={creating}>
                  {creating ? 'Creating…' : 'Create sprint'}
                </Button>
              </div>
            </div>

            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label>Preview</Label>
                <div className="flex gap-2">
                  <Button
                    variant="secondary"
                    onClick={handleCopy}
                    disabled={!releaseNotes?.markdown}
                  >
                    Copy
                  </Button>
                  <Button
                    variant="secondary"
                    onClick={handleDownload}
                    disabled={!selectedSprintId}
                  >
                    Download .md
                  </Button>
                </div>
              </div>
              <Textarea
                value={releaseNotes?.markdown ?? ''}
                readOnly
                placeholder="Select a sprint and click Generate…"
                className="min-h-[18rem] font-mono"
              />
            </div>
          </div>

          <DialogFooter>
            <Button variant="secondary" onClick={handleClose}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
);

export const SprintReleaseNotesDialog = defineModal<
  SprintReleaseNotesDialogProps,
  SprintReleaseNotesDialogResult
>(SprintReleaseNotesDialogImpl);
