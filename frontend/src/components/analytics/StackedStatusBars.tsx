type StatusPoint = {
  ts: unknown;
  todo: number;
  inprogress: number;
  inreview: number;
  done: number;
  cancelled: number;
};

function total(p: StatusPoint) {
  return p.todo + p.inprogress + p.inreview + p.done + p.cancelled;
}

const COLORS = {
  todo: 'hsl(var(--warning) / 0.55)',
  inprogress: 'hsl(var(--info) / 0.60)',
  inreview: 'hsl(var(--accent) / 0.75)',
  done: 'hsl(var(--success) / 0.65)',
  cancelled: 'hsl(var(--destructive) / 0.45)',
} as const;

export function StackedStatusBars({
  points,
  height = 140,
}: {
  points: StatusPoint[];
  height?: number;
}) {
  const w = 1000;
  const h = height;

  const maxTotal = Math.max(1, ...points.map(total));
  const barW = w / Math.max(1, points.length);

  return (
    <div className="relative rounded-md border bg-background/50 overflow-hidden">
      <svg
        viewBox={`0 0 ${w} ${h}`}
        className="w-full h-[160px]"
        role="img"
        aria-label="Stacked status chart"
      >
        {points.length === 0 ? (
          <text x="12" y="24" className="fill-muted-foreground text-xs">
            No data
          </text>
        ) : null}

        {points.map((p, i) => {
          const x = i * barW;
          const stacks: Array<[keyof typeof COLORS, number]> = [
            ['todo', p.todo],
            ['inprogress', p.inprogress],
            ['inreview', p.inreview],
            ['done', p.done],
            ['cancelled', p.cancelled],
          ];

          let y = h;
          return (
            <g key={i} transform={`translate(${x.toFixed(1)},0)`}>
              {stacks.map(([k, v]) => {
                if (!v) return null;
                const bh = (v / maxTotal) * (h - 10);
                y -= bh;
                return (
                  <rect
                    key={k}
                    x="1"
                    y={y.toFixed(1)}
                    width={(barW - 2).toFixed(1)}
                    height={bh.toFixed(1)}
                    fill={COLORS[k]}
                    rx="2"
                  />
                );
              })}
            </g>
          );
        })}
      </svg>

      <div className="absolute inset-x-0 bottom-0 flex flex-wrap gap-x-3 gap-y-1 px-3 py-2 bg-background/80 border-t">
        <LegendDot color={COLORS.todo} label="Todo" />
        <LegendDot color={COLORS.inprogress} label="In progress" />
        <LegendDot color={COLORS.inreview} label="In review" />
        <LegendDot color={COLORS.done} label="Done" />
        <LegendDot color={COLORS.cancelled} label="Cancelled" />
      </div>
    </div>
  );
}

function LegendDot({ color, label }: { color: string; label: string }) {
  return (
    <div className="flex items-center gap-2 text-xs text-muted-foreground">
      <span
        className="h-2 w-2 rounded-full border"
        style={{ backgroundColor: color }}
      />
      <span>{label}</span>
    </div>
  );
}

