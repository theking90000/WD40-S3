import { useTranslation } from "react-i18next";
import { Button } from "~/components/ui/button";
import { dashboardErrorKey } from "../api.ts";
import type { OrganizationDashboard } from "../api.ts";

export function DashboardStatus({ data, error, loading, refreshing, onRefresh }: {
  data?: OrganizationDashboard;
  error: Error | null;
  loading: boolean;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  if (loading) return <p role="status" className="sr-only">Chargement des statistiques…</p>;
  if (!error) return null;
  return (
    <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
      <p>{t($ => $.messages[dashboardErrorKey(error)])}{data && " Les dernières valeurs sont conservées."}</p>
      <Button type="button" variant="outline" size="sm" disabled={refreshing} onClick={onRefresh}>Réessayer</Button>
    </div>
  );
}
