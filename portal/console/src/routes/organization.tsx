import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { ArrowRightIcon, ChartColumnIcon, ChartPieIcon, ChevronRightIcon, DatabaseIcon, HistoryIcon, KeyRoundIcon, UsersIcon } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { organizationErrorMessage, organizationQuery } from "~/features/organizations/api.ts";
import { bucketErrorMessage, organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { BucketAccessBadge } from "~/features/organizations/buckets/components/access";
import { BucketDetails } from "~/features/organizations/buckets/components/bucket-details";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader } from "~/components/ui/card";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = { title: "Vue d'ensemble" };

export default function OrganizationHome() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? <Overview key={JSON.stringify(organizationQuery(user, organization.id).queryKey)} user={user} organization={organization} /> : null;
}

function MetricCard({ title, tag, status = "Pas disponible", children }: {
  title: string;
  tag?: string;
  status?: string;
  children?: ReactNode;
}) {
  return (
    <Card className="min-h-[130px] gap-0 p-4">
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-sans text-[11.5px] font-medium text-muted-foreground">{title}</h2>
        {tag && <span className="text-[10px] text-muted-foreground">{tag}</span>}
      </div>
      <p className="mt-3 font-display text-metric tabular-nums" aria-label="Valeur indisponible">—</p>
      <div className="mt-auto flex flex-wrap items-center justify-between gap-2 pt-3 text-[10px] text-muted-foreground">
        <span>{status}</span>{children}
      </div>
    </Card>
  );
}

function Unavailable({ icon: Icon, children }: { icon: LucideIcon; children: ReactNode }) {
  return (
    <div className="flex min-h-54 flex-1 flex-col items-center justify-center gap-3 px-5 py-8 text-center text-xs text-muted-foreground">
      <span className="flex size-10 items-center justify-center rounded-xl border bg-background"><Icon aria-hidden="true" className="size-4" strokeWidth={1.5} /></span>
      <p>{children}</p>
    </div>
  );
}

function Overview({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const details = useQuery(organizationQuery(user, organization.id));
  const buckets = useQuery(organizationBucketsQuery(user, organization.id));
  const [notice, setNotice] = useState("");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const heading = useRef<HTMLHeadingElement | null>(null);
  const detailTrigger = useRef<HTMLElement | null>(null);
  const counts = details.data?.counts;
  const recentBuckets = [...(buckets.data ?? [])].sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at) || a.name.localeCompare(b.name)).slice(0, 3);
  const selectedBucket = buckets.data?.find((bucket) => bucket.name === selectedName);
  const dateFormat = new Intl.DateTimeFormat("fr-BE", { dateStyle: "medium" });
  const numberFormat = new Intl.NumberFormat("fr-BE");
  const countLinks = [
    { path: "buckets", icon: DatabaseIcon, count: counts?.buckets, singular: "bucket", plural: "buckets" },
    { path: "credentials", icon: KeyRoundIcon, count: counts?.credentials, singular: "clé d'accès", plural: "clés d'accès" },
    { path: "members", icon: UsersIcon, count: counts?.members, singular: "membre", plural: "membres" },
  ];

  useEffect(() => {
    if (buckets.data && selectedName !== null && !selectedBucket) {
      setDetailsOpen(false);
      setSelectedName(null);
    }
  }, [buckets.data, selectedName, selectedBucket]);

  function openBucket(name: string, trigger: HTMLElement) {
    detailTrigger.current = trigger;
    setSelectedName(name);
    setDetailsOpen(true);
  }

  return (
    <section className="space-y-3.5">
      <div className="mb-6 flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 space-y-3">
          <div className="space-y-2">
            <p className="text-[10.5px] font-semibold text-muted-foreground">Vue d'ensemble</p>
            <h1 ref={heading} tabIndex={-1} className="break-words text-page-title">{organization.name}</h1>
            <p className="text-xs text-muted-foreground">Stockage, accès et activité de l'organisation.</p>
          </div>
          <div className="flex flex-wrap items-center gap-2" aria-busy={details.isPending}>
            {details.isPending ? <>
              <span role="status" className="sr-only">Chargement des compteurs…</span>
              {[0, 1, 2].map((item) => <Skeleton key={item} className="h-6 w-24 rounded-full" />)}
            </> : countLinks.map((item) => <Badge key={item.path} variant="outline" asChild>
              <Link to={`/${organization.id}/${item.path}`}>
                <item.icon aria-hidden="true" />
                {item.count === undefined ? "—" : numberFormat.format(item.count)} {item.count === 1 ? item.singular : item.plural}
              </Link>
            </Badge>)}
          </div>
          <Link to={`/${organization.id}/credentials#s3-connection`} className="inline-flex items-center gap-1.5 text-[10.5px] text-muted-foreground underline-offset-4 hover:text-foreground hover:underline">
            Configurer un client S3<ArrowRightIcon aria-hidden="true" className="size-3" />
          </Link>
        </div>
      </div>

      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{notice}</p>}
      {details.error && <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
        <p>{organizationErrorMessage(details.error)}{details.data && " Les compteurs affichés proviennent du dernier chargement réussi."}</p>
        <Button type="button" variant="outline" size="sm" disabled={details.isFetching} onClick={() => void details.refetch()}>Réessayer</Button>
      </div>}

      <div className="grid gap-3 min-[621px]:grid-cols-2 min-[1161px]:grid-cols-4">
        <MetricCard title="Stockage utilisé" />
        <MetricCard title="Objets stockés" tag="Total">
          {counts && <Link to={`/${organization.id}/buckets`} className="underline-offset-4 hover:text-foreground hover:underline">{numberFormat.format(counts.buckets)} bucket{counts.buckets === 1 ? "" : "s"}</Link>}
        </MetricCard>
        <MetricCard title="Requêtes · 24 h" tag="S3 API" />
        <MetricCard title="Volume de données transféré · 24 h" />
      </div>

      <div className="grid gap-3.5 min-[1161px]:grid-cols-[minmax(0,1.7fr)_minmax(275px,0.82fr)]">
        <Card className="min-w-0 gap-0 pb-0">
          <CardHeader className="flex flex-wrap items-start justify-between gap-3">
            <div className="space-y-1"><h2 className="text-card-title">Volume ajouté</h2><CardDescription>Go ajoutés par jour sur la période</CardDescription></div>
            <div role="group" aria-label="Période du graphique" title="Statistiques non disponibles" className="flex gap-0.5 rounded-lg border bg-background p-0.5">
              <Button type="button" variant="outline" size="xs" disabled>7 j</Button>
              <Button type="button" variant="ghost" size="xs" disabled>30 j</Button>
            </div>
          </CardHeader>
          <Unavailable icon={ChartColumnIcon}>Statistiques non disponibles</Unavailable>
        </Card>
        <Card className="min-w-0 gap-0 pb-0">
          <CardHeader><h2 className="text-card-title">Répartition</h2><CardDescription>Par bucket</CardDescription></CardHeader>
          <Unavailable icon={ChartPieIcon}>Répartition non disponible</Unavailable>
        </Card>
      </div>

      <div className="grid gap-3.5 min-[1161px]:grid-cols-[minmax(0,1.42fr)_minmax(300px,1fr)]">
        <Card className="min-w-0 gap-0 overflow-hidden py-0">
          <CardHeader className="flex flex-wrap items-center justify-between gap-3 border-b py-4">
            <div className="space-y-1"><h2 className="text-card-title">Derniers buckets créés</h2><CardDescription>Les plus récents dans l'organisation</CardDescription></div>
            <Link to={`/${organization.id}/buckets`} className="inline-flex items-center gap-1 text-[10.5px] text-muted-foreground underline-offset-4 hover:text-foreground hover:underline">Voir tout<ArrowRightIcon aria-hidden="true" className="size-3" /></Link>
          </CardHeader>
          {buckets.error && <div role="alert" className="space-y-3 border-b bg-destructive-soft p-4 text-xs text-destructive-strong">
            <p>{bucketErrorMessage(buckets.error)}{buckets.data && " Les données affichées proviennent du dernier chargement réussi. Les modifications sont suspendues."}</p>
            <Button type="button" variant="outline" size="sm" disabled={buckets.isFetching} onClick={() => void buckets.refetch()}>Réessayer</Button>
          </div>}
          {!buckets.data && !buckets.error ? <CardContent role="status" className="space-y-3 py-5">
            <span className="sr-only">Chargement des buckets…</span>
            {[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}
          </CardContent> : buckets.data && (recentBuckets.length === 0 ? <div className="flex min-h-52 flex-col items-center justify-center gap-3 p-5 text-center">
            <DatabaseIcon aria-hidden="true" className="size-7 text-muted-foreground" strokeWidth={1.5} />
            <p className="text-xs font-semibold">Aucun bucket</p>
            <p className="text-[10.5px] text-muted-foreground">Les buckets de cette organisation apparaîtront ici.</p>
          </div> : <Table className="table-fixed" aria-busy={buckets.isFetching}>
            <caption className="sr-only">Derniers buckets créés dans {organization.name}</caption>
            <TableHeader><TableRow>
              <TableHead scope="col">Bucket</TableHead>
              <TableHead scope="col" className="hidden w-28 min-[621px]:table-cell">Accès</TableHead>
              <TableHead scope="col" className="hidden w-28 min-[761px]:table-cell">Créé le</TableHead>
              <TableHead scope="col" className="w-9"><span className="sr-only">Ouvrir</span></TableHead>
            </TableRow></TableHeader>
            <TableBody>{recentBuckets.map((bucket) => <TableRow key={bucket.name} tabIndex={0} className="cursor-pointer focus-visible:-outline-offset-2"
              aria-label={`Ouvrir le bucket ${bucket.name}`} aria-haspopup="dialog" aria-expanded={detailsOpen && selectedName === bucket.name}
              onClick={(event) => openBucket(bucket.name, event.currentTarget)}
              onKeyDown={(event) => {
                if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
                  event.preventDefault();
                  openBucket(bucket.name, event.currentTarget);
                }
              }}>
              <TableCell>
                <div className="flex min-w-0 items-center gap-2.5">
                  <span className="hidden size-7 shrink-0 items-center justify-center rounded-lg border bg-background text-muted-foreground min-[621px]:flex"><DatabaseIcon aria-hidden="true" className="size-3.5" /></span>
                  <div className="min-w-0 space-y-1">
                    <p className="break-all font-semibold">{bucket.name}</p>
                    <p className="hidden truncate font-mono text-[10px] text-muted-foreground min-[621px]:block">s3://{bucket.name}</p>
                    <div className="min-[621px]:hidden"><BucketAccessBadge access={bucket.access} /></div>
                    <time className="block text-[10px] text-muted-foreground min-[761px]:hidden" dateTime={bucket.created_at}>Créé le {dateFormat.format(new Date(bucket.created_at))}</time>
                  </div>
                </div>
              </TableCell>
              <TableCell className="hidden min-[621px]:table-cell"><BucketAccessBadge access={bucket.access} /></TableCell>
              <TableCell className="hidden text-muted-foreground min-[761px]:table-cell"><time dateTime={bucket.created_at}>{dateFormat.format(new Date(bucket.created_at))}</time></TableCell>
              <TableCell className="text-muted-foreground"><ChevronRightIcon aria-hidden="true" className="size-3.5" /></TableCell>
            </TableRow>)}</TableBody>
          </Table>)}
        </Card>
        <Card className="min-w-0 gap-0 pb-0">
          <CardHeader><h2 className="text-card-title">Activité récente</h2><CardDescription>Derniers événements de l'organisation</CardDescription></CardHeader>
          <Unavailable icon={HistoryIcon}>Historique non disponible</Unavailable>
        </Card>
      </div>
      {selectedBucket && <BucketDetails key={selectedBucket.name} user={user} organization={organization} bucket={selectedBucket}
        open={detailsOpen} disabled={!!buckets.error} onOpenChange={setDetailsOpen}
        onUpdated={() => setNotice("Le niveau d'accès du bucket a été enregistré.")}
        restoreFocus={() => {
          if (detailTrigger.current?.isConnected) detailTrigger.current.focus();
          else heading.current?.focus();
        }} />}
    </section>
  );
}
