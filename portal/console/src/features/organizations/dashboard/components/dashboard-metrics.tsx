import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import { ArrowDownIcon, ArrowUpIcon } from "lucide-react";
import { Card } from "~/components/ui/card";
import { Skeleton } from "~/components/ui/skeleton";
import type { OrganizationDashboard } from "../api.ts";
import { useDashboardFormatters } from "~/features/organizations/dashboard/use-dashboard-formatters";

function MetricCard({ title, value, loading, children }: {
  title: string;
  value?: string;
  loading: boolean;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <Card className="min-h-[110px] min-w-0 gap-0 p-4" aria-busy={loading}>
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-sans text-[11.5px] font-medium text-muted-foreground">{title}</h2>
      </div>
      {loading ? <Skeleton aria-hidden="true" className="mt-3 h-9 w-28" /> :
        <p className="mt-3 break-words font-display text-metric tabular-nums" aria-label={value === undefined ? t($ => $.messages.valeurIndisponible) : undefined}>
          {value ?? "—"}
          {value !== undefined && children && <span className="ml-2 inline-flex items-center gap-1 align-baseline whitespace-nowrap font-sans text-[9px] font-normal text-muted-foreground">{children}</span>}
        </p>}
      {(loading || value === undefined) && <div className="mt-auto pt-3 text-[10px] text-muted-foreground">
        {loading ? t($ => $.messages.loading) : t($ => $.messages.unavailable)}
      </div>}
    </Card>
  );
}

export function DashboardMetrics({ data, loading }: {
  data?: OrganizationDashboard;
  loading: boolean;
}) {
  const { t } = useTranslation();
  const { formatBytes, formatCount } = useDashboardFormatters();
  return (
    <div className="grid gap-3 min-[621px]:grid-cols-2 min-[1161px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1.12fr)]">
      <MetricCard title={t($ => $.messages.stockageUtilise)} value={data && formatBytes(data.stored_bytes)} loading={loading} />
      <MetricCard title={t($ => $.messages.objetsStockes)} value={data && formatCount(data.stored_objects)} loading={loading} />
      <MetricCard title={t($ => $.messages.requetes24H)} value={data && formatCount(data.requests)} loading={loading} />
      <MetricCard title={t($ => $.messages.volumeDeDonneesTransfere24H)} value={data && formatBytes(data.transferred_bytes)} loading={loading}>
        {data && <>
          <span className="inline-flex items-center gap-0.5" aria-label={t($ => $.messages.televersesValue, { value: formatBytes(data.uploaded_bytes) })}><ArrowUpIcon aria-hidden="true" className="size-3" />{formatBytes(data.uploaded_bytes)}</span>
          <span className="inline-flex items-center gap-0.5" aria-label={t($ => $.messages.telechargesValue, { value: formatBytes(data.downloaded_bytes) })}><ArrowDownIcon aria-hidden="true" className="size-3" />{formatBytes(data.downloaded_bytes)}</span>
        </>}
      </MetricCard>
    </div>
  );
}
