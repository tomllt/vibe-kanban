type Bucket = {
  from_hours: number;
  to_hours: number;
  count: number;
};

function label(bucket: Bucket) {
  const to = bucket.to_hours >= 10_000 ? '∞' : `${bucket.to_hours}h`;
  return `${bucket.from_hours}h–${to}`;
}

export function Histogram({
  buckets,
  height = 120,
}: {
  buckets: Bucket[];
  height?: number;
}) {
  const w = 1000;
  const h = height;
  const max = Math.max(1, ...buckets.map((b) => b.count));
  const barW = w / Math.max(1, buckets.length);

  return (
    <div className="rounded-md border bg-background/50 overflow-hidden">
      <svg
        viewBox={`0 0 ${w} ${h}`}
        className="w-full h-[140px]"
        role="img"
        aria-label="Histogram"
      >
        {buckets.length === 0 ? (
          <text x="12" y="24" className="fill-muted-foreground text-xs">
            No samples
          </text>
        ) : null}

        {buckets.map((b, i) => {
          const x = i * barW;
          const bh = (b.count / max) * (h - 24);
          return (
            <g key={i}>
              <rect
                x={x + 2}
                y={h - 18 - bh}
                width={barW - 4}
                height={bh}
                rx="3"
                fill="hsl(var(--foreground) / 0.18)"
                stroke="hsl(var(--border))"
              />
            </g>
          );
        })}
      </svg>

      <div className="px-3 py-2 border-t bg-background/80">
        <div className="flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-muted-foreground">
          {buckets.map((b, i) => (
            <span key={i} className="whitespace-nowrap">
              {label(b)}: {b.count}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

