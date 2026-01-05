import { useId, useMemo } from 'react';

type Point = {
  ts: unknown;
  value: number;
};

function asDate(ts: unknown): Date {
  return ts instanceof Date ? ts : new Date(ts as string);
}

export function LineChart({
  points,
  height = 120,
  stroke = 'hsl(var(--foreground))',
  fill = 'hsl(var(--foreground) / 0.10)',
}: {
  points: Point[];
  height?: number;
  stroke?: string;
  fill?: string;
}) {
  const gradientId = useId();
  const { path, areaPath } = useMemo(() => {
    if (points.length < 2) return { path: '', areaPath: '' };

    const values = points.map((p) => p.value);
    const min = Math.min(...values);
    const max = Math.max(...values);
    const span = Math.max(1, max - min);

    const w = 1000;
    const h = height;
    const padY = 10;

    const xStep = w / Math.max(1, points.length - 1);
    const y = (v: number) =>
      padY + ((max - v) / span) * (h - padY * 2);

    let d = '';
    for (let i = 0; i < points.length; i++) {
      const x = i * xStep;
      const yy = y(points[i].value);
      d += `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${yy.toFixed(1)} `;
    }

    const area =
      `${d}L ${w.toFixed(1)} ${(h - padY).toFixed(1)} ` +
      `L 0 ${(h - padY).toFixed(1)} Z`;

    return { path: d.trim(), areaPath: area.trim() };
  }, [points, height]);

  const dateRangeLabel = useMemo(() => {
    if (!points.length) return '';
    const start = asDate(points[0].ts);
    const end = asDate(points[points.length - 1].ts);
    return `${start.toLocaleDateString()} → ${end.toLocaleDateString()}`;
  }, [points]);

  return (
    <div className="w-full">
      <div className="text-xs text-muted-foreground mb-2">{dateRangeLabel}</div>
      <div className="relative rounded-md border bg-background/50 overflow-hidden">
        <svg
          viewBox={`0 0 1000 ${height}`}
          className="w-full h-[140px]"
          role="img"
          aria-label="Line chart"
        >
          <defs>
            <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={fill} />
              <stop offset="100%" stopColor="transparent" />
            </linearGradient>
          </defs>

          {areaPath ? (
            <path d={areaPath} fill={`url(#${gradientId})`} />
          ) : (
            <text x="12" y="24" className="fill-muted-foreground text-xs">
              Not enough data
            </text>
          )}

          {path ? (
            <path
              d={path}
              fill="none"
              stroke={stroke}
              strokeWidth="2"
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ) : null}
        </svg>
      </div>
    </div>
  );
}
