import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { CheckIcon, CopyIcon, DatabaseIcon, PlusIcon, RefreshCwIcon, SearchIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { BucketAccess } from "~/features/organizations/buckets/api.ts";
import { bucketErrorMessage, organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { BucketAccessBadge } from "~/features/organizations/buckets/components/access";
import { BucketDetails } from "~/features/organizations/buckets/components/bucket-details";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = { title: "Buckets" };

export default function Buckets() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? (
    <BucketsPage key={JSON.stringify(organizationBucketsQuery(user, organization.id).queryKey)} user={user} organization={organization} />
  ) : null;
}

function BucketsPage({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const { data: buckets, error, isFetching, refetch } = useQuery(organizationBucketsQuery(user, organization.id));
  const [search, setSearch] = useState("");
  const [access, setAccess] = useState<BucketAccess | "ALL">("ALL");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [notice, setNotice] = useState("");
  const [copyError, setCopyError] = useState("");
  const [copiedName, setCopiedName] = useState<string | null>(null);
  const detailTrigger = useRef<HTMLElement | null>(null);
  const needle = search.trim().toLocaleLowerCase("fr");
  const filtered = buckets?.filter((bucket) => bucket.name.toLocaleLowerCase("fr").includes(needle)
    && (access === "ALL" || bucket.access === access)) ?? [];
  const selectedBucket = buckets?.find((bucket) => bucket.name === selectedName);
  const filteredView = !!needle || access !== "ALL";
  const dateFormat = new Intl.DateTimeFormat("fr-BE", { dateStyle: "medium" });

  useEffect(() => {
    if (copiedName === null) return;
    const timer = setTimeout(() => setCopiedName(null), 2000);
    return () => clearTimeout(timer);
  }, [copiedName]);

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
    setNotice("");
    setCopyError("");
    try {
      await navigator.clipboard.writeText(name);
      setCopiedName(name);
    } catch {
      setCopyError("Copie impossible. Ouvrez le bucket pour sélectionner et copier son nom manuellement.");
    }
  }

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">Stockage</p>
          <h1 className="text-page-title">Buckets</h1>
          <p className="text-muted-foreground">Buckets de {organization.name}.</p>
          {buckets && <Badge variant="outline">{buckets.length} bucket{buckets.length > 1 ? "s" : ""}</Badge>}
        </div>
        {organization.access === "OWNER" && (
          <Button asChild>
            <Link to={`/${organization.id}/buckets/new`}><PlusIcon aria-hidden="true" />Créer un bucket</Link>
          </Button>
        )}
      </div>
      <p className="rounded-lg border bg-background p-3 text-xs text-muted-foreground">Les niveaux d'accès sont enregistrés dans le portail. Leur application côté S3 n'est pas encore active.</p>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-0 flex-1 basis-48 sm:max-w-sm">
          <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)} autoComplete="off"
            placeholder="Filtrer les buckets…" aria-label="Rechercher un bucket" className="pl-9" disabled={!buckets} />
        </div>
        <div className="w-full min-[461px]:w-48">
          <NativeSelect value={access} disabled={!buckets} aria-label="Filtrer par accès" onChange={(event) => {
            const value = event.target.value;
            if (value === "ALL" || value === "PRIVATE" || value === "PUBLIC" || value === "PUBLIC_LIST") setAccess(value);
          }}>
            <NativeSelectOption value="ALL">Tous les accès</NativeSelectOption>
            <NativeSelectOption value="PRIVATE">Privé</NativeSelectOption>
            <NativeSelectOption value="PUBLIC">Public</NativeSelectOption>
            <NativeSelectOption value="PUBLIC_LIST">Public avec listing</NativeSelectOption>
          </NativeSelect>
        </div>
        {buckets && <span className="text-[10.5px] text-muted-foreground" aria-live="polite">{filtered.length} résultat{filtered.length > 1 ? "s" : ""}</span>}
        <Button type="button" variant="outline" size="sm" className="ml-auto" disabled={isFetching} aria-busy={isFetching} onClick={() => void refetch()}>
          <RefreshCwIcon aria-hidden="true" />{isFetching ? "Actualisation…" : "Actualiser"}
        </Button>
      </div>
      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{notice}</p>}
      {copyError && <p role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{copyError}</p>}
      {error && (
        <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-4 text-xs text-destructive-strong">
          <p>{bucketErrorMessage(error)}</p>
          <Button type="button" variant="outline" size="sm" onClick={() => void refetch()} disabled={isFetching}>Réessayer</Button>
        </div>
      )}
      {(buckets || !error) && (
        <div className="overflow-hidden rounded-xl border bg-surface" aria-busy={isFetching}>
          {!buckets ? (
            <div role="status" className="space-y-4 p-5">
              <span className="sr-only">Chargement des buckets…</span>
              {[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}
            </div>
          ) : filtered.length === 0 ? (
            <div className="flex flex-col items-center gap-3 px-5 py-12 text-center">
              {filteredView ? <SearchIcon aria-hidden="true" className="size-8 text-muted-foreground" /> : <DatabaseIcon aria-hidden="true" className="size-8 text-muted-foreground" />}
              <h2 className="text-card-title">{filteredView ? "Aucun bucket trouvé" : "Aucun bucket"}</h2>
              <p className="text-xs text-muted-foreground">{filteredView ? "Essayez un autre nom ou un autre niveau d'accès." : "Les buckets de cette organisation apparaîtront ici."}</p>
              {filteredView && <Button type="button" variant="outline" size="sm" onClick={clearFilters}>Réinitialiser les filtres</Button>}
            </div>
          ) : (
            <Table className="table-fixed">
              <caption className="sr-only">Buckets de {organization.name}</caption>
              <TableHeader>
                <TableRow>
                  <TableHead scope="col">Bucket</TableHead>
                  <TableHead scope="col" className="hidden w-40 min-[621px]:table-cell">Accès</TableHead>
                  <TableHead scope="col" className="hidden w-36 min-[761px]:table-cell">Créé le</TableHead>
                  <TableHead scope="col" className="w-16"><span className="sr-only">Actions</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((bucket) => (
                  <TableRow key={bucket.name} tabIndex={0} className="cursor-pointer focus-visible:-outline-offset-2"
                    aria-label={`Ouvrir le bucket ${bucket.name}`} aria-haspopup="dialog" aria-expanded={detailsOpen && selectedName === bucket.name}
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
                        <time className="min-[761px]:hidden" dateTime={bucket.created_at}>Créé le {dateFormat.format(new Date(bucket.created_at))}</time>
                      </div>
                    </TableCell>
                    <TableCell className="hidden min-[621px]:table-cell"><BucketAccessBadge access={bucket.access} /></TableCell>
                    <TableCell className="hidden text-muted-foreground min-[761px]:table-cell"><time dateTime={bucket.created_at}>{dateFormat.format(new Date(bucket.created_at))}</time></TableCell>
                    <TableCell className="text-right">
                      <Button type="button" variant="ghost" size="icon-sm" title={copiedName === bucket.name ? "Copié" : "Copier le nom"}
                        aria-label={copiedName === bucket.name ? `Nom du bucket ${bucket.name} copié` : `Copier le nom du bucket ${bucket.name}`}
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
            <span>{filtered.length} bucket{filtered.length > 1 ? "s" : ""} affiché{filtered.length > 1 ? "s" : ""}</span>
            <span>Seuls les propriétaires peuvent créer un bucket ou modifier son accès.</span>
          </div>}
        </div>
      )}
      {selectedBucket && <BucketDetails key={selectedBucket.name} user={user} organization={organization} bucket={selectedBucket}
        open={detailsOpen} disabled={!!error} onOpenChange={setDetailsOpen}
        onUpdated={() => { setNotice("Le niveau d'accès du bucket a été enregistré."); setCopyError(""); }}
        restoreFocus={() => { if (detailTrigger.current?.isConnected) detailTrigger.current.focus(); }} />}
    </section>
  );
}
