import { useLocale } from "~/features/i18n/use-locale";
import { useTranslation } from "react-i18next";
import { ChartPieIcon } from "lucide-react";
import { Button } from "~/components/ui/button";
import { Card, CardContent, CardHeader } from "~/components/ui/card";
import { Progress } from "~/components/ui/progress";
import { Skeleton } from "~/components/ui/skeleton";
import { bucketErrorKey } from "../../buckets/api.ts";
import type { BucketWithUsage } from "../../buckets/api.ts";
import { useDashboardFormatters } from "~/features/organizations/dashboard/use-dashboard-formatters";

export function BucketDistribution({ data, loading, error, refreshing, onRefresh }: {
  data?: BucketWithUsage[];
  loading: boolean;
  error: Error | null;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const locale = useLocale();
  const { formatBytes, formatPercentage } = useDashboardFormatters();
  const buckets = [...(data ?? [])].sort((a, b) => b.stored_bytes - a.stored_bytes || a.name.localeCompare(b.name, locale));
  const total = buckets.reduce((sum, bucket) => sum + bucket.stored_bytes, 0);
  return (
    <Card className="min-w-0 gap-0 pb-0" aria-busy={loading}>
      <CardHeader><h2 className="text-card-title">{t($ => $.messages.repartitionParBucket)}</h2></CardHeader>
      {error && <div role="alert" className="mx-4.5 mb-4 space-y-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
        <p>{t($ => $.messages[bucketErrorKey(error)])}{data && t($ => $.messages.lesDernieresValeursSontConservees)}</p>
        <Button type="button" variant="outline" size="sm" disabled={refreshing} onClick={onRefresh}>{t($ => $.messages.retry)}</Button>
      </div>}
      {loading ? <CardContent className="space-y-5 py-5">
        <span role="status" className="sr-only">{t($ => $.messages.chargementDeLaRepartition)}</span>
        {[0, 1, 2].map((item) => <div key={item} className="space-y-2"><Skeleton className="h-4 w-2/3" /><Skeleton className="h-[5px] w-full" /></div>)}
      </CardContent> : !data || buckets.length === 0 ? <CardContent className="flex min-h-54 flex-1 flex-col items-center justify-center gap-3 py-8 text-center text-xs text-muted-foreground">
        <ChartPieIcon aria-hidden="true" className="size-5" strokeWidth={1.5} />
        <p>{!data ? t($ => $.messages.repartitionIndisponible) : t($ => $.messages.cetteOrganisationNeContientAucunBucket)}</p>
      </CardContent> : <CardContent className="space-y-4 py-5">
        {total === 0 && <p className="text-[10.5px] text-muted-foreground">{t($ => $.messages.aucunStockageMesurePourLeMoment)}</p>}
        <ul className="space-y-4">
          {buckets.map((bucket) => {
            const ratio = total > 0 ? bucket.stored_bytes / total : 0;
            const percentage = formatPercentage(ratio);
            return <li key={bucket.name} className="space-y-2">
              <div className="flex items-start justify-between gap-3 text-[10.5px]">
                <span className="min-w-0 break-all font-semibold">{bucket.name}</span>
                <span className="shrink-0 text-right tabular-nums">{formatBytes(bucket.stored_bytes)}</span>
              </div>
              <Progress value={ratio * 100} aria-label={t($ => $.messages.partDuStockageDuBucketName, { name: bucket.name })} aria-valuetext={t($ => $.messages.storageShare, { percentage })} />
            </li>;
          })}
        </ul>
      </CardContent>}
    </Card>
  );
}
