import { Badge } from "../atoms/badge";

type ChangeSummaryProps = {
  name: string;
  kind: string;
  summary: string;
  impact: string;
};

export function ChangeSummary({ name, kind, summary, impact }: ChangeSummaryProps) {
  return (
    <div className="flex flex-col gap-3 border-b border-border px-5 py-4 sm:flex-row sm:items-center sm:justify-between">
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <strong className="truncate text-sm">{name}</strong>
          <Badge tone="changed">{kind}</Badge>
        </div>
        <p className="mt-1 text-sm text-muted-foreground">{summary}</p>
      </div>
      <Badge tone="warning">{impact}</Badge>
    </div>
  );
}
