import { ChartPieIcon } from "lucide-react";
import { Card, CardContent, CardHeader } from "~/components/ui/card";
import { Progress } from "~/components/ui/progress";
import { Skeleton } from "~/components/ui/skeleton";
import type { OrganizationDashboard } from "../api.ts";
import { formatBytes, formatPercentage } from "../format.ts";

export function BucketDistribution({ data, loading }: { data?: OrganizationDashboard; loading: boolean }) {
  const buckets = [...(data?.buckets ?? [])].sort((a, b) => b.stored_bytes - a.stored_bytes || a.name.localeCompare(b.name, "fr-BE"));
  return (
    <Card className="min-w-0 gap-0 pb-0" aria-busy={loading}>
      <CardHeader><h2 className="text-card-title">Répartition par bucket</h2></CardHeader>
      {loading ? <CardContent className="space-y-5 py-5">
        {[0, 1, 2].map((item) => <div key={item} className="space-y-2"><Skeleton className="h-4 w-2/3" /><Skeleton className="h-[5px] w-full" /></div>)}
      </CardContent> : !data || buckets.length === 0 ? <CardContent className="flex min-h-54 flex-1 flex-col items-center justify-center gap-3 py-8 text-center text-xs text-muted-foreground">
        <ChartPieIcon aria-hidden="true" className="size-5" strokeWidth={1.5} />
        <p>{!data ? "Répartition indisponible" : "Cette organisation ne contient aucun bucket."}</p>
      </CardContent> : <CardContent className="space-y-4 py-5">
        {data.stored_bytes === 0 && <p className="text-[10.5px] text-muted-foreground">Aucun stockage mesuré pour le moment.</p>}
        <ul className="space-y-4">
          {buckets.map((bucket) => {
            const ratio = data.stored_bytes > 0 ? bucket.stored_bytes / data.stored_bytes : 0;
            const percentage = formatPercentage(ratio);
            return <li key={bucket.name} className="space-y-2">
              <div className="flex items-start justify-between gap-3 text-[10.5px]">
                <span className="min-w-0 break-all font-semibold">{bucket.name}</span>
                <span className="shrink-0 text-right tabular-nums">{formatBytes(bucket.stored_bytes)}</span>
              </div>
              <Progress value={ratio * 100} aria-label={`Part du stockage du bucket ${bucket.name}`} aria-valuetext={`${percentage} du stockage total`} />
            </li>;
          })}
        </ul>
      </CardContent>}
    </Card>
  );
}
