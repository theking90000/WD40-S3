import type { Message } from "~/features/i18n/message";
import { useLocale } from "~/features/i18n/use-locale";
import { useTranslation } from "react-i18next";
import { routeTitle } from "~/features/i18n/route-titles";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { ArrowRightIcon, ChevronRightIcon, DatabaseIcon, HistoryIcon, KeyRoundIcon, UsersIcon } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { organizationErrorKey, organizationQuery } from "~/features/organizations/api.ts";
import { bucketErrorKey, organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { BucketAccessBadge } from "~/features/organizations/buckets/components/access";
import { BucketDetails } from "~/features/organizations/buckets/components/bucket-details";
import { organizationDashboardQuery } from "~/features/organizations/dashboard/api.ts";
import { BucketDistribution } from "~/features/organizations/dashboard/components/bucket-distribution";
import { DashboardMetrics } from "~/features/organizations/dashboard/components/dashboard-metrics";
import { DashboardStatus } from "~/features/organizations/dashboard/components/dashboard-status";
import { UploadsChart } from "~/features/organizations/dashboard/components/uploads-chart";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader } from "~/components/ui/card";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = routeTitle("overview");

export default function OrganizationHome() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? <Overview key={JSON.stringify(organizationQuery(user, organization.id).queryKey)} user={user} organization={organization} /> : null;
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
  const { t } = useTranslation();
  const locale = useLocale();
  const details = useQuery(organizationQuery(user, organization.id));
  const buckets = useQuery({
    ...organizationBucketsQuery(user, organization.id),
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
  const dashboard = useQuery(organizationDashboardQuery(user, organization.id));
  const [notice, setNotice] = useState<Message | null>(null);
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const heading = useRef<HTMLHeadingElement | null>(null);
  const detailTrigger = useRef<HTMLElement | null>(null);
  const counts = details.data?.counts;
  const recentBuckets = [...(buckets.data ?? [])].sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at) || a.name.localeCompare(b.name, locale)).slice(0, 3);
  const selectedBucket = buckets.data?.find((bucket) => bucket.name === selectedName);
  const dateFormat = new Intl.DateTimeFormat(locale, { dateStyle: "medium" });
  const numberFormat = new Intl.NumberFormat(locale);
  const countLinks = [
    { path: "buckets", icon: DatabaseIcon, count: counts?.buckets, messageKey: "overviewBucketCount", unavailableKey: "overviewBucketsUnavailable" },
    { path: "credentials", icon: KeyRoundIcon, count: counts?.credentials, messageKey: "overviewCredentialCount", unavailableKey: "overviewCredentialsUnavailable" },
    { path: "members", icon: UsersIcon, count: counts?.members, messageKey: "overviewMemberCount", unavailableKey: "overviewMembersUnavailable" },
  ] as const;

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
            <p className="text-[10.5px] font-semibold text-muted-foreground">{t($ => $.messages.overview)}</p>
            <h1 ref={heading} tabIndex={-1} className="break-words text-page-title">{organization.name}</h1>
            <p className="text-xs text-muted-foreground">{t($ => $.messages.stockageAccesEtActiviteDeLOrganisation)}</p>
          </div>
          <div className="flex flex-wrap items-center gap-2" aria-busy={details.isPending}>
            {details.isPending ? <>
              <span role="status" className="sr-only">{t($ => $.messages.chargementDesCompteurs)}</span>
              {[0, 1, 2].map((item) => <Skeleton key={item} className="h-6 w-24 rounded-full" />)}
            </> : countLinks.map((item) => <Badge key={item.path} variant="outline" asChild>
              <Link to={`/${organization.id}/${item.path}`}>
                <item.icon aria-hidden="true" />
                {item.count === undefined ? t($ => $.messages[item.unavailableKey])
                  : t($ => $.messages[item.messageKey], { count: item.count, formattedCount: numberFormat.format(item.count) })}
              </Link>
            </Badge>)}
          </div>
        </div>
      </div>

      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{t($ => $.messages[notice.key], notice.values)}</p>}
      {details.error && <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
        <p>{t($ => $.messages[organizationErrorKey(details.error)])}{details.data && t($ => $.messages.lesCompteursAffichesProviennentDuDernierChargementReussi)}</p>
        <Button type="button" variant="outline" size="sm" disabled={details.isFetching} onClick={() => void details.refetch()}>{t($ => $.messages.retry)}</Button>
      </div>}

      <DashboardStatus data={dashboard.data} error={dashboard.error} loading={dashboard.isPending}
        refreshing={dashboard.isFetching} onRefresh={() => void dashboard.refetch()} />
      <DashboardMetrics data={dashboard.data} loading={dashboard.isPending} />

      <div className="grid gap-3.5 min-[1161px]:grid-cols-[minmax(0,1.7fr)_minmax(275px,0.82fr)]">
        <UploadsChart user={user} organizationId={organization.id} />
        <BucketDistribution data={buckets.data} loading={buckets.isPending} error={buckets.error}
          refreshing={buckets.isFetching} onRefresh={() => void buckets.refetch()} />
      </div>

      <div className="grid gap-3.5 min-[1161px]:grid-cols-[minmax(0,1.42fr)_minmax(300px,1fr)]">
        <Card className="min-w-0 gap-0 overflow-hidden py-0">
          <CardHeader className="flex flex-wrap items-center justify-between gap-3 border-b py-4">
            <div className="space-y-1"><h2 className="text-card-title">{t($ => $.messages.derniersBucketsCrees)}</h2><CardDescription>{t($ => $.messages.lesPlusRecentsDansLOrganisation)}</CardDescription></div>
            <Link to={`/${organization.id}/buckets`} className="inline-flex items-center gap-1 text-[10.5px] text-muted-foreground underline-offset-4 hover:text-foreground hover:underline">{t($ => $.messages.voirTout)}<ArrowRightIcon aria-hidden="true" className="size-3" /></Link>
          </CardHeader>
          {buckets.error && <div role="alert" className="space-y-3 border-b bg-destructive-soft p-4 text-xs text-destructive-strong">
            <p>{t($ => $.messages[bucketErrorKey(buckets.error)])}{buckets.data && t($ => $.messages.lesDonneesAfficheesProviennentDuDernierChargementReussiLes2)}</p>
            <Button type="button" variant="outline" size="sm" disabled={buckets.isFetching} onClick={() => void buckets.refetch()}>{t($ => $.messages.retry)}</Button>
          </div>}
          {!buckets.data && !buckets.error ? <CardContent role="status" className="space-y-3 py-5">
            <span className="sr-only">{t($ => $.messages.chargementDesBuckets)}</span>
            {[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}
          </CardContent> : buckets.data && (recentBuckets.length === 0 ? <div className="flex min-h-52 flex-col items-center justify-center gap-3 p-5 text-center">
            <DatabaseIcon aria-hidden="true" className="size-7 text-muted-foreground" strokeWidth={1.5} />
            <p className="text-xs font-semibold">{t($ => $.messages.aucunBucket)}</p>
            <p className="text-[10.5px] text-muted-foreground">{t($ => $.messages.lesBucketsDeCetteOrganisationApparaitrontIci)}</p>
          </div> : <Table className="table-fixed" aria-busy={buckets.isFetching}>
            <caption className="sr-only">{t($ => $.messages.derniersBucketsCreesDansName, { name: organization.name })}</caption>
            <TableHeader><TableRow>
              <TableHead scope="col">{t($ => $.messages.bucket)}</TableHead>
              <TableHead scope="col" className="hidden w-28 min-[621px]:table-cell">{t($ => $.messages.access)}</TableHead>
              <TableHead scope="col" className="hidden w-28 min-[761px]:table-cell">{t($ => $.messages.createdAt)}</TableHead>
              <TableHead scope="col" className="w-9"><span className="sr-only">{t($ => $.messages.open)}</span></TableHead>
            </TableRow></TableHeader>
            <TableBody>{recentBuckets.map((bucket) => <TableRow key={bucket.name} tabIndex={0} className="cursor-pointer focus-visible:-outline-offset-2"
              aria-label={t($ => $.messages.ouvrirLeBucketName, { name: bucket.name })} aria-haspopup="dialog" aria-expanded={detailsOpen && selectedName === bucket.name}
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
                    <time className="block text-[10px] text-muted-foreground min-[761px]:hidden" dateTime={bucket.created_at}>{t($ => $.messages.creeLeValue, { value: dateFormat.format(new Date(bucket.created_at)) })}</time>
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
          <CardHeader><h2 className="text-card-title">{t($ => $.messages.activiteRecente)}</h2><CardDescription>{t($ => $.messages.derniersEvenementsDeLOrganisation)}</CardDescription></CardHeader>
          <Unavailable icon={HistoryIcon}>{t($ => $.messages.historiqueNonDisponible)}</Unavailable>
        </Card>
      </div>
      {selectedBucket && <BucketDetails key={selectedBucket.name} user={user} organization={organization} bucket={selectedBucket}
        open={detailsOpen} disabled={!!buckets.error} onOpenChange={setDetailsOpen}
        onUpdated={() => setNotice({ key: "leNiveauDAccesDuBucketAEteEnregistre" })}
        restoreFocus={() => {
          if (detailTrigger.current?.isConnected) detailTrigger.current.focus();
          else heading.current?.focus();
        }} />}
    </section>
  );
}
