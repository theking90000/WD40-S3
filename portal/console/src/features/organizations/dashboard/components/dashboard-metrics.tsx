import type { ReactNode } from "react";
import { Card } from "~/components/ui/card";
import { Skeleton } from "~/components/ui/skeleton";
import type { OrganizationDashboard } from "../api.ts";
import { formatBytes, formatCount } from "../format.ts";

function MetricCard({ title, value, loading, children }: {
  title: string;
  value?: string;
  loading: boolean;
  children?: ReactNode;
}) {
  return (
    <Card className="min-h-[130px] min-w-0 gap-0 p-4" aria-busy={loading}>
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-sans text-[11.5px] font-medium text-muted-foreground">{title}</h2>
      </div>
      {loading ? <Skeleton aria-hidden="true" className="mt-3 h-9 w-28" /> :
        <p className="mt-3 break-words font-display text-metric tabular-nums" aria-label={value === undefined ? "Valeur indisponible" : undefined}>{value ?? "—"}</p>}
      {(loading || value === undefined || children) && <div className="mt-auto space-y-1 pt-3 text-[10px] text-muted-foreground">
        {loading ? "Chargement…" : value === undefined ? "Indisponible" : children}
      </div>}
    </Card>
  );
}

export function DashboardMetrics({ data, loading }: {
  data?: OrganizationDashboard;
  loading: boolean;
}) {
  return (
    <div className="grid gap-3 min-[621px]:grid-cols-2 min-[1161px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1.12fr)]">
      <MetricCard title="Stockage utilisé" value={data && formatBytes(data.stored_bytes)} loading={loading} />
      <MetricCard title="Objets stockés" value={data && formatCount(data.stored_objects)} loading={loading} />
      <MetricCard title="Requêtes · 24 h" value={data && formatCount(data.requests)} loading={loading} />
      <MetricCard title="Volume de données transféré · 24 h" value={data && formatBytes(data.transferred_bytes)} loading={loading}>
        {data && <><p>Téléversés : {formatBytes(data.uploaded_bytes)}</p><p>Téléchargés : {formatBytes(data.downloaded_bytes)}</p></>}
      </MetricCard>
    </div>
  );
}
