import { useLocale } from "~/features/i18n/use-locale";
import type { Message } from "~/features/i18n/message";
import { useTranslation } from "react-i18next";
import { routeTitle } from "~/features/i18n/route-titles";
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { CheckIcon, CopyIcon, DatabaseIcon, PlusIcon, RefreshCwIcon, SearchIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { BucketAccess } from "~/features/organizations/buckets/api.ts";
import { bucketErrorKey, organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { BucketAccessBadge } from "~/features/organizations/buckets/components/access";
import { BucketDetails } from "~/features/organizations/buckets/components/bucket-details";
import { useDashboardFormatters } from "~/features/organizations/dashboard/use-dashboard-formatters";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = routeTitle("buckets");

export default function Buckets() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? (
    <BucketsPage key={JSON.stringify(organizationBucketsQuery(user, organization.id).queryKey)} user={user} organization={organization} />
  ) : null;
}

function BucketsPage({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const { formatBytes, formatCount } = useDashboardFormatters();
  const { data: buckets, error, isFetching, refetch } = useQuery({
    ...organizationBucketsQuery(user, organization.id),
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
  const [search, setSearch] = useState("");
  const [access, setAccess] = useState<BucketAccess | "ALL">("ALL");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [notice, setNotice] = useState<Message | null>(null);
  const [copyError, setCopyError] = useState<Message | null>(null);
  const [copiedName, setCopiedName] = useState<string | null>(null);
  const detailTrigger = useRef<HTMLElement | null>(null);
  const heading = useRef<HTMLHeadingElement | null>(null);
  const needle = search.trim().toLocaleLowerCase(locale);
  const filtered = buckets?.filter((bucket) => bucket.name.toLocaleLowerCase(locale).includes(needle)
    && (access === "ALL" || bucket.access === access)) ?? [];
  const selectedBucket = buckets?.find((bucket) => bucket.name === selectedName);
  const filteredView = !!needle || access !== "ALL";
  const dateFormat = new Intl.DateTimeFormat(locale, { dateStyle: "medium" });

  useEffect(() => {
    if (copiedName === null) return;
    const timer = setTimeout(() => setCopiedName(null), 2000);
    return () => clearTimeout(timer);
  }, [copiedName]);

  useEffect(() => {
    if (buckets && selectedName !== null && !selectedBucket) {
      setDetailsOpen(false);
      setSelectedName(null);
    }
  }, [buckets, selectedName, selectedBucket]);

  function openBucket(name: string, trigger: HTMLElement) {
    detailTrigger.current = trigger;
    setSelectedName(name);
    setDetailsOpen(true);
  }

  function clearFilters() {
    setSearch("");
    setAccess("ALL");
  }

  async function copyName(name: string) {
    setNotice(null);
    setCopyError(null);
    try {
      await navigator.clipboard.writeText(name);
      setCopiedName(name);
    } catch {
      setCopyError({ key: "copieImpossibleOuvrezLeBucketPourSelectionnerEtCopier" });
    }
  }

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">{t($ => $.messages.storage)}</p>
          <h1 ref={heading} tabIndex={-1} className="text-page-title">{t($ => $.messages.buckets)}</h1>
          <p className="text-muted-foreground">{t($ => $.messages.bucketsDeName, { name: organization.name })}</p>
          {buckets && <Badge variant="outline">{t($ => $.messages.bucketCount, { count: buckets.length })}</Badge>}
        </div>
        {organization.access === "OWNER" && (
          <Button asChild>
            <Link to={`/${organization.id}/buckets/new`}><PlusIcon aria-hidden="true" />{t($ => $.messages.createBucket)}</Link>
          </Button>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-0 flex-1 basis-48 sm:max-w-sm">
          <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)} autoComplete="off"
            placeholder={t($ => $.messages.filtrerLesBuckets)} aria-label={t($ => $.messages.rechercherUnBucket)} className="pl-9" disabled={!buckets} />
        </div>
        <div className="w-full min-[461px]:w-48">
          <NativeSelect value={access} disabled={!buckets} aria-label={t($ => $.messages.filtrerParAcces)} onChange={(event) => {
            const value = event.target.value;
            if (value === "ALL" || value === "PRIVATE" || value === "PUBLIC" || value === "PUBLIC_LIST") setAccess(value);
          }}>
            <NativeSelectOption value="ALL">{t($ => $.messages.tousLesAcces)}</NativeSelectOption>
            <NativeSelectOption value="PRIVATE">{t($ => $.messages.prive)}</NativeSelectOption>
            <NativeSelectOption value="PUBLIC">{t($ => $.messages.public)}</NativeSelectOption>
            <NativeSelectOption value="PUBLIC_LIST">{t($ => $.messages.publicAvecListing)}</NativeSelectOption>
          </NativeSelect>
        </div>
        {buckets && <span className="text-[10.5px] text-muted-foreground" aria-live="polite">{t($ => $.messages.resultCount, { count: filtered.length })}</span>}
        <Button type="button" variant="outline" size="sm" className="ml-auto" disabled={isFetching} aria-busy={isFetching} onClick={() => void refetch()}>
          <RefreshCwIcon aria-hidden="true" />{isFetching ? t($ => $.messages.refreshing) : t($ => $.messages.refresh)}
        </Button>
      </div>
      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{t($ => $.messages[notice.key], notice.values)}</p>}
      <span role="status" className="sr-only">{copiedName ? t($ => $.messages.nomDuBucketCopiednameCopie, { copiedName: copiedName }) : ""}</span>
      {copyError && <p role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{t($ => $.messages[copyError.key], copyError.values)}</p>}
      {error && (
        <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-4 text-xs text-destructive-strong">
          <p>{t($ => $.messages[bucketErrorKey(error)])}{buckets && t($ => $.messages.lesDonneesAfficheesProviennentDuDernierChargementReussiLes)}</p>
          <Button type="button" variant="outline" size="sm" onClick={() => void refetch()} disabled={isFetching}>{t($ => $.messages.retry)}</Button>
        </div>
      )}
      {(buckets || !error) && (
        <div className="overflow-hidden rounded-xl border bg-surface" aria-busy={isFetching}>
          {!buckets ? (
            <div role="status" className="space-y-4 p-5">
              <span className="sr-only">{t($ => $.messages.chargementDesBuckets)}</span>
              {[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}
            </div>
          ) : filtered.length === 0 ? (
            <div className="flex flex-col items-center gap-3 px-5 py-12 text-center">
              {filteredView ? <SearchIcon aria-hidden="true" className="size-8 text-muted-foreground" /> : <DatabaseIcon aria-hidden="true" className="size-8 text-muted-foreground" />}
              <h2 className="text-card-title">{filteredView ? t($ => $.messages.aucunBucketTrouve) : t($ => $.messages.aucunBucket)}</h2>
              <p className="text-xs text-muted-foreground">{filteredView ? t($ => $.messages.essayezUnAutreNomOuUnAutreNiveauD) : t($ => $.messages.lesBucketsDeCetteOrganisationApparaitrontIci)}</p>
              {filteredView && <Button type="button" variant="outline" size="sm" onClick={clearFilters}>{t($ => $.messages.reinitialiserLesFiltres)}</Button>}
            </div>
          ) : (
            <Table className="table-fixed">
              <caption className="sr-only">{t($ => $.messages.bucketsDeName2, { name: organization.name })}</caption>
              <TableHeader>
                <TableRow>
                  <TableHead scope="col">{t($ => $.messages.bucket)}</TableHead>
                  <TableHead scope="col" className="hidden w-36 text-right min-[1161px]:table-cell">{t($ => $.messages.stockageUtilise)}</TableHead>
                  <TableHead scope="col" className="hidden w-28 text-right min-[1161px]:table-cell">{t($ => $.messages.objets)}</TableHead>
                  <TableHead scope="col" className="hidden w-40 min-[621px]:table-cell">{t($ => $.messages.access)}</TableHead>
                  <TableHead scope="col" className="hidden w-36 min-[761px]:table-cell">{t($ => $.messages.createdAt)}</TableHead>
                  <TableHead scope="col" className="w-16"><span className="sr-only">{t($ => $.messages.actions)}</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((bucket) => (
                  <TableRow key={bucket.name} tabIndex={0} className="cursor-pointer focus-visible:-outline-offset-2"
                    aria-label={t($ => $.messages.ouvrirLeBucketName, { name: bucket.name })} aria-haspopup="dialog" aria-expanded={detailsOpen && selectedName === bucket.name}
                    onClick={(event) => openBucket(bucket.name, event.currentTarget)}
                    onKeyDown={(event) => {
                      if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
                        event.preventDefault();
                        openBucket(bucket.name, event.currentTarget);
                      }
                    }}>
                    <TableCell>
                      <div className="flex max-w-full items-center gap-2.5 text-left">
                        <span className="hidden size-7 shrink-0 items-center justify-center rounded-lg border bg-background text-muted-foreground min-[621px]:flex"><DatabaseIcon aria-hidden="true" className="size-3.5" /></span>
                        <span className="min-w-0 break-all font-semibold">{bucket.name}</span>
                      </div>
                      <div className="mt-1 flex flex-wrap items-center gap-2 text-[10px] text-muted-foreground">
                        <span className="min-[621px]:hidden"><BucketAccessBadge access={bucket.access} /></span>
                        <time className="min-[761px]:hidden" dateTime={bucket.created_at}>{t($ => $.messages.creeLeValue, { value: dateFormat.format(new Date(bucket.created_at)) })}</time>
                      </div>
                      <dl className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[10px] text-muted-foreground min-[1161px]:hidden">
                        <div className="flex gap-1"><dt>{t($ => $.messages.stockage)}</dt><dd className="font-medium text-foreground tabular-nums">{formatBytes(bucket.stored_bytes)}</dd></div>
                        <div className="flex gap-1"><dt>{t($ => $.messages.objets2)}</dt><dd className="font-medium text-foreground tabular-nums">{formatCount(bucket.stored_objects)}</dd></div>
                      </dl>
                    </TableCell>
                    <TableCell className="hidden text-right tabular-nums min-[1161px]:table-cell">{formatBytes(bucket.stored_bytes)}</TableCell>
                    <TableCell className="hidden text-right tabular-nums min-[1161px]:table-cell">{formatCount(bucket.stored_objects)}</TableCell>
                    <TableCell className="hidden min-[621px]:table-cell"><BucketAccessBadge access={bucket.access} /></TableCell>
                    <TableCell className="hidden text-muted-foreground min-[761px]:table-cell"><time dateTime={bucket.created_at}>{dateFormat.format(new Date(bucket.created_at))}</time></TableCell>
                    <TableCell className="text-right">
                      <Button type="button" variant="ghost" size="icon-sm" title={copiedName === bucket.name ? t($ => $.messages.copied) : t($ => $.messages.copierLeNom)}
                        aria-label={copiedName === bucket.name ? t($ => $.messages.nomDuBucketNameCopie, { name: bucket.name }) : t($ => $.messages.copierLeNomDuBucketName, { name: bucket.name })}
                        onClick={(event) => { event.stopPropagation(); void copyName(bucket.name); }}>
                        {copiedName === bucket.name ? <CheckIcon aria-hidden="true" className="size-3.5" /> : <CopyIcon aria-hidden="true" className="size-3.5" />}
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
          {buckets && <div className="flex flex-wrap items-center justify-between gap-2 border-t px-3.5 py-3 text-[10px] text-muted-foreground">
            <span>{t($ => $.messages.displayedBuckets, { count: filtered.length })}</span>
            <span>{t($ => $.messages.seulsLesProprietairesPeuventCreerUnBucketOuModifier)}</span>
          </div>}
        </div>
      )}
      {selectedBucket && <BucketDetails key={selectedBucket.name} user={user} organization={organization} bucket={selectedBucket}
        open={detailsOpen} disabled={!!error} onOpenChange={setDetailsOpen}
        onUpdated={() => { setNotice({ key: "leNiveauDAccesDuBucketAEteEnregistre" }); setCopyError(null); }}
        restoreFocus={() => {
          if (detailTrigger.current?.isConnected) detailTrigger.current.focus();
          else heading.current?.focus();
        }} />}
    </section>
  );
}
