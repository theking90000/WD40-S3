import { useTranslation } from "react-i18next";
import { useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChartColumnIcon } from "lucide-react";
import { Button } from "~/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader } from "~/components/ui/card";
import { Skeleton } from "~/components/ui/skeleton";
import type { CurrentUser } from "~/features/auth/session.ts";
import { dashboardErrorKey, organizationUploadsQuery } from "../api.ts";
import type { OrganizationUploads, UploadPeriod, UploadPoint } from "../api.ts";
import { formatBytes, formatChartDate, formatCount, formatUploadDate } from "../format.ts";

export function UploadsChart({ user, organizationId }: { user: CurrentUser; organizationId: string }) {
  const { t } = useTranslation();
  const [period, setPeriod] = useState<UploadPeriod>("7d");
  const [selectedStart, setSelectedStart] = useState<number | null>(null);
  const uploads = useQuery(organizationUploadsQuery(user, organizationId, period));
  const headingId = useId();
  const descriptionId = useId();
  const selected = uploads.data?.points.find((point) => point.start === selectedStart) ?? uploads.data?.points.at(-1);

  return (
    <Card className="min-w-0 gap-0 pb-0" aria-labelledby={headingId}>
      <CardHeader className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <h2 id={headingId} className="text-card-title">Volume ajouté</h2>
          <CardDescription id={descriptionId} className="flex min-h-4 flex-wrap items-baseline gap-x-2 gap-y-1">
            {selected ? <>
              <time dateTime={new Date(selected.start * 1000).toISOString()}>{formatUploadDate(selected.start, uploads.data?.period.resolution_seconds)}</time>
              <span className="font-semibold text-foreground tabular-nums">{formatBytes(selected.uploaded_bytes)}</span>
              {!selected.complete && <span>{period === "1d" ? "Période en cours" : "Journée en cours"}</span>}
            </> : "—"}
          </CardDescription>
        </div>
        <div role="group" aria-label="Période du graphique" className="flex gap-0.5 rounded-lg border bg-background p-0.5">
          {(["1d", "7d", "30d"] as const).map((value) => <Button key={value} type="button" size="xs"
            variant={period === value ? "outline" : "ghost"} aria-pressed={period === value}
            onClick={() => { setPeriod(value); setSelectedStart(null); }}>{value === "1d" ? "24h" : value === "7d" ? "7 j" : "30 j"}</Button>)}
        </div>
      </CardHeader>
      <CardContent className="flex min-h-64 flex-1 flex-col py-4" aria-busy={uploads.isFetching}>
        {uploads.error && <div role="alert" className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
          <p>{t($ => $.messages[dashboardErrorKey(uploads.error)])}{uploads.data && " Les dernières valeurs sont conservées."}</p>
          <Button type="button" variant="outline" size="sm" disabled={uploads.isFetching}
            onClick={() => void uploads.refetch()}>Réessayer</Button>
        </div>}
        {uploads.isPending ? <div role="status" className="mt-auto space-y-6">
          <span className="sr-only">Chargement du volume téléversé…</span>
          <Skeleton className="h-4 w-48" />
          <Skeleton className="h-40 w-full" />
        </div> : uploads.data && (uploads.data.points.every((point) => point.uploaded_bytes === 0)
          ? <div className="flex min-h-56 flex-1 flex-col items-center justify-center gap-3 text-center text-xs text-muted-foreground">
            <ChartColumnIcon aria-hidden="true" className="size-5" strokeWidth={1.5} />
            <p>Aucun volume téléversé sur cette période.</p>
          </div>
          : <DailyUploads key={period} data={uploads.data} selected={selected} onSelect={setSelectedStart} descriptionId={descriptionId} />)}
      </CardContent>
    </Card>
  );
}

function DailyUploads({ data, selected, onSelect, descriptionId }: {
  data: OrganizationUploads;
  selected?: UploadPoint;
  onSelect: (start: number | null) => void;
  descriptionId: string;
}) {
  const peak = Math.max(...data.points.map((point) => point.uploaded_bytes), 1);
  const magnitude = 10 ** Math.floor(Math.log10(peak / 4));
  const step = Math.max(1, Math.ceil(peak / 4 / magnitude) * magnitude);
  const maximum = step * 4;
  const width = 1000;
  const height = 160;
  const slot = width / data.points.length;
  const barWidth = Math.min(70, slot * 0.62);
  const resolution = data.period.resolution_seconds;
  const intraday = resolution < 86_400;
  const pendingLabel = intraday ? "Période en cours" : "Journée en cours";

  return (
    <div className="mt-auto">
      <div className="grid grid-cols-[3.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
        <div aria-hidden="true" className="relative h-40 text-right text-[10px] text-muted-foreground tabular-nums">
          {[0, 1, 2, 3, 4].map((tick) => <span key={tick} className="absolute right-0 -translate-y-1/2 whitespace-nowrap"
            style={{ top: `${100 - tick * 25}%` }}>{formatBytes(step * tick, maximum)}</span>)}
        </div>
        <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="h-40 w-full overflow-visible"
          role="group" aria-label={intraday ? "Volume téléversé par période de 30 minutes en UTC" : "Volume téléversé par jour en UTC"} aria-describedby={descriptionId}>
          <g aria-hidden="true">{[0, 1, 2, 3, 4].map((tick) => <line key={tick} x1={0} x2={width}
            y1={height - tick * height / 4} y2={height - tick * height / 4}
            className="stroke-border" vectorEffect="non-scaling-stroke" />)}</g>
          {data.points.map((point, index) => {
            const x = index * slot;
            const barHeight = point.uploaded_bytes / maximum * height;
            const label = `${formatUploadDate(point.start, resolution)}, ${formatCount(point.uploaded_bytes)} octets${point.complete ? "" : `, ${pendingLabel.toLowerCase()}`}`;
            return <g key={point.start} tabIndex={0} role="button" aria-label={label}
              className="group cursor-pointer outline-none"
              onPointerEnter={() => onSelect(point.start)} onFocus={() => onSelect(point.start)}
              onClick={() => onSelect(point.start)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onSelect(point.start);
                }
                if (event.key === "Escape") onSelect(null);
              }}>
              <title>{label}</title>
              <rect x={x + 1} y={0} width={slot - 2} height={height} rx={4}
                className="fill-transparent stroke-transparent group-focus-visible:stroke-ring" vectorEffect="non-scaling-stroke" />
              <rect x={x + (slot - barWidth) / 2} y={height - barHeight} width={barWidth} height={barHeight} rx={3}
                className="pointer-events-none fill-chart-1" opacity={point.complete ? (selected?.start === point.start ? 1 : 0.8) : 0.45} />
            </g>;
          })}
        </svg>
        <div aria-hidden="true" className="relative col-start-2 h-6 text-[10px] text-muted-foreground">
          {data.points.map((point, index) => {
            const last = data.points.length - 1;
            const mobile = index === 0 || index === Math.round(last / 2) || index === last;
            if (data.points.length !== 7 && ![0, 0.25, 0.5, 0.75, 1].some((ratio) => index === Math.round(last * ratio))) return null;
            return <span key={point.start} className={`absolute whitespace-nowrap ${mobile ? "" : "hidden min-[761px]:block"}`}
              style={{ left: index === 0 ? "0%" : index === last ? "100%" : `${(index + 0.5) / data.points.length * 100}%`,
                transform: `translateX(${index === 0 ? "0" : index === last ? "-100%" : "-50%"})` }}>
              {formatChartDate(point.start, resolution)}
            </span>;
          })}
        </div>
      </div>
      <table className="sr-only" aria-label="Volumes téléversés par période">
        <thead><tr><th scope="col">{intraday ? "Date et heure (UTC)" : "Date"}</th><th scope="col">Octets téléversés</th><th scope="col">État</th></tr></thead>
        <tbody>{data.points.map((point) => <tr key={point.start}>
          <th scope="row">{formatUploadDate(point.start, resolution)}</th><td>{formatCount(point.uploaded_bytes)}</td>
          <td>{point.complete ? (intraday ? "Période terminée" : "Journée terminée") : pendingLabel}</td>
        </tr>)}</tbody>
      </table>
    </div>
  );
}
