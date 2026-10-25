import { useLocale } from "~/features/i18n/use-locale";
import type { Message } from "~/features/i18n/message";
import { useTranslation } from "react-i18next";
import { routeTitle } from "~/features/i18n/route-titles";
import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { KeyRoundIcon, PlusIcon, RefreshCwIcon, SearchIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { CredentialAccessType } from "~/features/organizations/credentials/api.ts";
import { credentialErrorKey, organizationCredentialsQuery } from "~/features/organizations/credentials/api.ts";
import { CredentialPermissions, CredentialScope } from "~/features/organizations/credentials/components/access-form";
import { CopyValue } from "~/components/copy-value";
import { S3Connection } from "~/components/s3-connection";
import { DeleteCredentialDialog } from "~/features/organizations/credentials/components/delete-credential";
import { EditCredentialDialog } from "~/features/organizations/credentials/components/edit-credential";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = routeTitle("credentials");

export default function Credentials() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? <CredentialsPage key={JSON.stringify(organizationCredentialsQuery(user, organization.id).queryKey)} user={user} organization={organization} /> : null;
}

function CredentialsPage({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const { data: credentials, error, isFetching, refetch } = useQuery(organizationCredentialsQuery(user, organization.id));
  const [search, setSearch] = useState("");
  const [accessType, setAccessType] = useState<CredentialAccessType | "ANY">("ANY");
  const [notice, setNotice] = useState<Message | null>(null);
  const heading = useRef<HTMLHeadingElement | null>(null);
  const needle = search.trim().toLocaleLowerCase(locale);
  const owner = organization.access === "OWNER";
  const filtered = credentials?.filter((credential) => (accessType === "ANY" || credential.access_type === accessType)
    && (credential.key_id.toLocaleLowerCase(locale).includes(needle) || credential.bucket_names.some((name) => name.toLocaleLowerCase(locale).includes(needle)))) ?? [];
  const filteredView = !!needle || accessType !== "ANY";
  const dateFormat = new Intl.DateTimeFormat(locale, { dateStyle: "medium" });

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">{t($ => $.messages.storage)}</p>
          <h1 ref={heading} tabIndex={-1} className="text-page-title">{t($ => $.messages.credentials)}</h1>
          <p className="text-muted-foreground">{t($ => $.messages.clesDAccesDeName, { name: organization.name })}</p>
          {credentials && <Badge variant="outline">{t($ => $.messages.keyCount, { count: credentials.length })}</Badge>}
        </div>
        {organization.access === "OWNER" && (
          <Button asChild>
            <Link to={`/${organization.id}/credentials/new`}><PlusIcon aria-hidden="true" />{t($ => $.messages.creerUneCle)}</Link>
          </Button>
        )}
      </div>
      <div id="s3-connection" className="scroll-mt-5"><S3Connection /></div>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-0 flex-1 basis-48 sm:max-w-sm">
          <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)} className="pl-9" autoComplete="off"
            placeholder={t($ => $.messages.cleOuBucketSelectionne)} aria-label={t($ => $.messages.rechercherUneCleOuUnBucketSelectionne)} disabled={!credentials} />
        </div>
        <div className="w-full min-[461px]:w-48">
          <NativeSelect value={accessType} disabled={!credentials} aria-label={t($ => $.messages.filtrerParPortee)} onChange={(event) => {
            const value = event.target.value;
            if (value === "ANY" || value === "ALL" || value === "RESTRICTED") setAccessType(value);
          }}>
            <NativeSelectOption value="ANY">{t($ => $.messages.toutesLesPortees)}</NativeSelectOption>
            <NativeSelectOption value="ALL">{t($ => $.messages.tousLesBuckets)}</NativeSelectOption>
            <NativeSelectOption value="RESTRICTED">{t($ => $.messages.bucketsSelectionnes)}</NativeSelectOption>
          </NativeSelect>
        </div>
        {credentials && <span className="text-[10.5px] text-muted-foreground" aria-live="polite">{t($ => $.messages.resultCount, { count: filtered.length })}</span>}
        <Button type="button" variant="outline" size="sm" className="ml-auto" disabled={isFetching} aria-busy={isFetching} onClick={() => void refetch()}><RefreshCwIcon aria-hidden="true" />{isFetching ? t($ => $.messages.refreshing) : t($ => $.messages.refresh)}</Button>
      </div>
      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{t($ => $.messages[notice.key], notice.values)}</p>}
      {error && <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-4 text-xs text-destructive-strong">
        <p>{t($ => $.messages[credentialErrorKey(error)])}{credentials && t($ => $.messages.lesDonneesAfficheesProviennentDuDernierChargementReussiLes)}</p><Button type="button" variant="outline" size="sm" disabled={isFetching} onClick={() => void refetch()}>{t($ => $.messages.retry)}</Button>
      </div>}
      {(credentials || !error) && <div className="overflow-hidden rounded-xl border bg-surface" aria-busy={isFetching}>
        {!credentials ? <div role="status" className="space-y-4 p-5"><span className="sr-only">{t($ => $.messages.chargementDesCles)}</span>{[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}</div>
          : filtered.length === 0 ? <div className="flex flex-col items-center gap-3 px-5 py-12 text-center">
            <KeyRoundIcon aria-hidden="true" className="size-8 text-muted-foreground" />
            <h2 className="text-card-title">{filteredView ? t($ => $.messages.aucuneCleTrouvee) : t($ => $.messages.aucuneCleDAcces)}</h2>
            <p className="text-xs text-muted-foreground">{filteredView ? t($ => $.messages.modifiezLaRechercheOuLaPorteeSelectionnee) : t($ => $.messages.lesClesDeCetteOrganisationApparaitrontIci)}</p>
            {filteredView && <Button type="button" variant="outline" size="sm" onClick={() => { setSearch(""); setAccessType("ANY"); }}>{t($ => $.messages.reinitialiserLesFiltres)}</Button>}
          </div> : <Table className="table-fixed">
            <caption className="sr-only">{t($ => $.messages.clesDAccesDeName2, { name: organization.name })}</caption>
            <TableHeader><TableRow>
              <TableHead scope="col">{t($ => $.messages.identifiant)}</TableHead>
              <TableHead scope="col" className="hidden w-40 min-[761px]:table-cell">{t($ => $.messages.portee)}</TableHead>
              <TableHead scope="col" className="hidden w-44 min-[1161px]:table-cell">{t($ => $.messages.permissions)}</TableHead>
              <TableHead scope="col" className="hidden w-32 min-[961px]:table-cell">{t($ => $.messages.creeeLe)}</TableHead>
              <TableHead scope="col" className="hidden w-36 min-[1161px]:table-cell">{t($ => $.messages.derniereUtilisation)}</TableHead>
              {owner && <TableHead scope="col" className="w-20"><span className="sr-only">{t($ => $.messages.actions)}</span></TableHead>}
            </TableRow></TableHeader>
            <TableBody>{filtered.map((credential) => <TableRow key={credential.key_id}>
              <TableCell>
                <div className="flex flex-wrap items-center gap-1"><code className="min-w-0 select-all break-all font-mono text-[10.5px]">{credential.key_id}</code><CopyValue value={credential.key_id} label={t($ => $.messages.copierLIdentifiant)} compact /></div>
                <div className="mt-1 space-y-1.5 text-[10px] text-muted-foreground">
                  <div className="min-[761px]:hidden"><CredentialScope credential={credential} /></div>
                  <div className="min-[1161px]:hidden"><span className="sr-only">{t($ => $.messages.permissions2)}</span><CredentialPermissions credential={credential} /></div>
                  <time className="min-[961px]:hidden" dateTime={credential.created_at}>{t($ => $.messages.creeeLeValue, { value: dateFormat.format(new Date(credential.created_at)) })}</time>
                  <p className="min-[1161px]:hidden">{t($ => $.messages.derniereUtilisationNonDisponible)}</p>
                </div>
              </TableCell>
              <TableCell className="hidden min-[761px]:table-cell"><CredentialScope credential={credential} /></TableCell>
              <TableCell className="hidden min-[1161px]:table-cell"><CredentialPermissions credential={credential} /></TableCell>
              <TableCell className="hidden text-muted-foreground min-[961px]:table-cell"><time dateTime={credential.created_at}>{dateFormat.format(new Date(credential.created_at))}</time></TableCell>
              <TableCell className="hidden text-muted-foreground min-[1161px]:table-cell">{t($ => $.messages.notAvailable)}</TableCell>
              {owner && <TableCell className="text-right"><div className="flex items-center justify-end gap-1">
                <EditCredentialDialog user={user} organizationId={organization.id} credential={credential} disabled={!!error} restoreFocus={() => heading.current?.focus()} onUpdated={(updated) => setNotice({ key: "lesAccesDeLaCleKeyIdOntEte", values: { key_id: updated.key_id } })} />
                <DeleteCredentialDialog user={user} organizationId={organization.id} credential={credential} disabled={!!error} restoreFocus={() => heading.current?.focus()} onDeleted={(keyId) => setNotice({ key: "laCleKeyidAEteSupprimeeDuPortail", values: { keyId: keyId } })} />
              </div></TableCell>}
            </TableRow>)}</TableBody>
          </Table>}
        {credentials && <div className="flex flex-wrap items-center justify-between gap-2 border-t px-3.5 py-3 text-[10px] text-muted-foreground"><span>{t($ => $.messages.displayedKeys, { count: filtered.length })}</span><span>{t($ => $.messages.seulsLesProprietairesPeuventCreerModifierOuSupprimerLes)}</span></div>}
      </div>}
    </section>
  );
}
