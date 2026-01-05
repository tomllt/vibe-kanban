import { type ReactNode, useMemo, useState } from 'react';
import { useProject } from '@/contexts/ProjectContext';
import { Loader } from '@/components/ui/loader';
import { Button } from '@/components/ui/button';
import { NewCard, NewCardContent, NewCardHeader } from '@/components/ui/new-card';
import {
  useProjectBurndown,
  useProjectCfd,
  useProjectCycleTime,
  useProjectDevEx,
} from '@/hooks';
import { LineChart } from '@/components/analytics/LineChart';
import { StackedStatusBars } from '@/components/analytics/StackedStatusBars';
import { Histogram } from '@/components/analytics/Histogram';
import { cn } from '@/lib/utils';

function asNumber(n: unknown): number {
  return typeof n === 'number' ? n : Number(n);
}

export function AnalyticsDashboard() {
  const { projectId, project, isLoading: projectLoading } = useProject();
  const [days, setDays] = useState<7 | 30 | 90>(30);

  const burndown = useProjectBurndown({
    projectId: projectId || '',
    days,
  });
  const cfd = useProjectCfd({ projectId: projectId || '', days });
  const cycleTime = useProjectCycleTime({ projectId: projectId || '', days });
  const devex = useProjectDevEx({ projectId: projectId || '', days });

  const isLoading =
    projectLoading ||
    burndown.isLoading ||
    cfd.isLoading ||
    cycleTime.isLoading ||
    devex.isLoading;

  const anyError =
    burndown.error || cfd.error || cycleTime.error || devex.error;

  const burndownPoints = useMemo(() => {
    const points = burndown.data?.points ?? [];
    return points.map((p) => ({
      ts: p.ts as unknown,
      value: asNumber(p.remaining),
    }));
  }, [burndown.data]);

  const devexTurns = useMemo(() => {
    const points = devex.data?.agent_turns ?? [];
    return points.map((p) => ({ ts: p.ts as unknown, value: asNumber(p.value) }));
  }, [devex.data]);

  const devexRuns = useMemo(() => {
    const points = devex.data?.agent_runs ?? [];
    return points.map((p) => ({ ts: p.ts as unknown, value: asNumber(p.value) }));
  }, [devex.data]);

  if (!projectId) {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="text-sm text-muted-foreground">No project selected.</div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="h-full flex items-center justify-center">
        <Loader message="Loading analytics..." size={32} />
      </div>
    );
  }

  if (anyError) {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="text-sm text-destructive">
          Failed to load analytics. Try again.
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="min-h-full bg-diagonal-lines">
        <div className="mx-auto max-w-6xl px-4 py-6 space-y-4">
          <NewCard className="border border-border bg-background/70 backdrop-blur">
            <NewCardHeader
              className="bg-background/60"
              actions={
                <div className="flex items-center gap-2">
                  <RangeButton active={days === 7} onClick={() => setDays(7)}>
                    7d
                  </RangeButton>
                  <RangeButton active={days === 30} onClick={() => setDays(30)}>
                    30d
                  </RangeButton>
                  <RangeButton active={days === 90} onClick={() => setDays(90)}>
                    90d
                  </RangeButton>
                </div>
              }
            >
              <div className="flex items-baseline justify-between gap-4">
                <div className="min-w-0">
                  <div className="text-sm text-muted-foreground">Vibe analytics</div>
                  <div className="text-lg font-semibold truncate">
                    {project?.name ?? 'Project'}
                  </div>
                </div>
                <div className="hidden sm:flex text-xs text-muted-foreground">
                  Derived from task status transitions + agent activity.
                </div>
              </div>
            </NewCardHeader>

            <NewCardContent className="p-4">
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <Section
                  title="Burndown"
                  subtitle="Remaining work (todo + in progress + review)"
                >
                  <LineChart
                    points={burndownPoints}
                    stroke="hsl(var(--warning))"
                    fill="hsl(var(--warning) / 0.18)"
                  />
                </Section>

                <Section
                  title="CFD"
                  subtitle="Daily distribution by task status"
                >
                  <StackedStatusBars
                    points={(cfd.data?.points ?? []).map((p) => ({
                      ts: p.ts as unknown,
                      todo: asNumber(p.todo),
                      inprogress: asNumber(p.inprogress),
                      inreview: asNumber(p.inreview),
                      done: asNumber(p.done),
                      cancelled: asNumber(p.cancelled),
                    }))}
                  />
                </Section>

                <Section
                  title="Cycle time"
                  subtitle="First in progress → done (completed within range)"
                >
                  <div className="grid grid-cols-2 gap-3 mb-3">
                    <Stat
                      label="Samples"
                      value={formatInt(cycleTime.data?.sample_size ?? 0)}
                    />
                    <Stat
                      label="Mean (h)"
                      value={format1(cycleTime.data?.mean_hours)}
                    />
                    <Stat label="P50 (h)" value={format1(cycleTime.data?.p50_hours)} />
                    <Stat label="P90 (h)" value={format1(cycleTime.data?.p90_hours)} />
                  </div>
                  <Histogram
                    buckets={(cycleTime.data?.histogram ?? []).map((b) => ({
                      from_hours: asNumber(b.from_hours),
                      to_hours: asNumber(b.to_hours),
                      count: asNumber(b.count),
                    }))}
                  />
                </Section>

                <Section title="DevEx" subtitle="Agent activity + hotspots">
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                      <div className="flex items-center justify-between mb-2">
                        <div className="text-xs text-muted-foreground">
                          Agent turns / day
                        </div>
                        <div className="text-xs text-muted-foreground">
                          Tasks touched: {formatInt(devex.data?.tasks_touched ?? 0)}
                        </div>
                      </div>
                      <LineChart
                        points={devexTurns}
                        stroke="hsl(var(--info))"
                        fill="hsl(var(--info) / 0.15)"
                      />
                    </div>
                    <div>
                      <div className="text-xs text-muted-foreground mb-2">
                        Coding agent runs / day
                      </div>
                      <LineChart
                        points={devexRuns}
                        stroke="hsl(var(--success))"
                        fill="hsl(var(--success) / 0.12)"
                      />
                    </div>
                  </div>

                  <div className="mt-4 space-y-3">
                    {(devex.data?.hotspots ?? []).map((repo) => (
                      <div key={repo.repo_id} className="border rounded-md bg-background/40">
                        <div className="px-3 py-2 border-b flex items-center justify-between">
                          <div className="text-sm font-medium truncate">
                            {repo.repo_display_name}
                          </div>
                          <div className="text-xs text-muted-foreground">
                            Hotspots (top {Math.min(20, repo.files.length)})
                          </div>
                        </div>
                        <div className="p-3 grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-2">
                          {repo.files.slice(0, 12).map((f) => (
                            <div
                              key={f.path}
                              className="flex items-center justify-between gap-3 text-xs"
                            >
                              <span className="truncate">{f.path}</span>
                              <span className="text-muted-foreground tabular-nums">
                                {formatInt(f.commit_count)}×
                              </span>
                            </div>
                          ))}
                          {repo.files.length === 0 ? (
                            <div className="text-xs text-muted-foreground">
                              No git stats available for this repo path.
                            </div>
                          ) : null}
                        </div>
                      </div>
                    ))}
                  </div>
                </Section>
              </div>
            </NewCardContent>
          </NewCard>
        </div>
      </div>
    </div>
  );
}

function RangeButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <Button
      variant={active ? 'default' : 'ghost'}
      size="sm"
      className={cn('h-8 px-3 text-xs', !active && 'border')}
      onClick={onClick}
    >
      {children}
    </Button>
  );
}

function Section({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  return (
    <div className="rounded-lg border bg-muted/70">
      <div className="px-3 py-3 border-b bg-background/50">
        <div className="flex items-baseline justify-between gap-3">
          <div className="font-semibold">{title}</div>
          <div className="text-xs text-muted-foreground text-right">{subtitle}</div>
        </div>
      </div>
      <div className="p-3">{children}</div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="rounded-md border bg-background/50 px-3 py-2">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      <div className="text-sm font-semibold tabular-nums">{value}</div>
    </div>
  );
}

function format1(v: unknown) {
  const n = typeof v === 'number' ? v : Number(v);
  if (!Number.isFinite(n)) return '0.0';
  return n.toFixed(1);
}

function formatInt(v: unknown) {
  if (typeof v === 'bigint') return v.toString();
  const n = typeof v === 'number' ? v : Number(v);
  if (!Number.isFinite(n)) return '0';
  return Math.trunc(n).toLocaleString();
}
