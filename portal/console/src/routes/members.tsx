import { useLocale } from "~/features/i18n/use-locale";
import type { Message } from "~/features/i18n/message";
import { useTranslation } from "react-i18next";
import { routeTitle } from "~/features/i18n/route-titles";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RefreshCwIcon, SearchIcon, UsersIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { memberErrorKey, organizationMembersQuery } from "~/features/organizations/members/api.ts";
import { AddMemberDialog } from "~/features/organizations/members/components/add-member";
import { EditMemberDialog } from "~/features/organizations/members/components/edit-member";
import { RemoveMemberDialog } from "~/features/organizations/members/components/remove-member";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = routeTitle("members");

export default function Members() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? (
    <MembersPage key={JSON.stringify(organizationMembersQuery(user, organization.id).queryKey)} user={user} organization={organization} />
  ) : null;
}

function MembersPage({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const { data: members, error, isFetching, refetch } = useQuery(organizationMembersQuery(user, organization.id));
  const [search, setSearch] = useState("");
  const [notice, setNotice] = useState<Message | null>(null);
  const [mutationError, setMutationError] = useState<Error | null>(null);
  const owner = organization.access === "OWNER";

  const needle = search.trim().toLocaleLowerCase(locale);
  const filtered = members?.filter(({ email }) => email.toLocaleLowerCase(locale).includes(needle)) ?? [];
  const dateFormat = new Intl.DateTimeFormat(locale, { dateStyle: "medium" });

  function showNotice(message: Message) {
    setMutationError(null);
    setNotice(message);
  }

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">{t($ => $.messages.organization)}</p>
          <h1 className="text-page-title">{t($ => $.messages.members)}</h1>
          <p className="text-muted-foreground">{t($ => $.messages.controlezQuiAccedeALOrganisationEtAuxRessources)}</p>
          {members && <Badge variant="outline">{t($ => $.messages.memberCount, { count: members.length })}</Badge>}
        </div>
        {owner && <AddMemberDialog user={user} organizationId={organization.id} disabled={!members || !!error}
          onAdded={(email) => showNotice({ key: "emailAEteAjouteALOrganisation", values: { email: email } })} onError={setMutationError} />}
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-0 flex-1 sm:max-w-sm">
          <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)}
            placeholder={t($ => $.messages.rechercherUnMembre)} aria-label={t($ => $.messages.rechercherUnMembre2)} autoComplete="off" className="pl-9" disabled={!members} />
        </div>
        {members && <span className="text-[10.5px] text-muted-foreground" aria-live="polite">{t($ => $.messages.resultCount, { count: filtered.length })}</span>}
        <Button type="button" variant="outline" size="sm" className="ml-auto" onClick={() => void refetch()} disabled={isFetching} aria-busy={isFetching}>
          <RefreshCwIcon aria-hidden="true" />{isFetching ? t($ => $.messages.refreshing) : t($ => $.messages.refresh)}
        </Button>
      </div>

      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{t($ => $.messages[notice.key], notice.values)}</p>}
      {mutationError && <p role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{t($ => $.messages[memberErrorKey(mutationError)])}</p>}
      {error && (
        <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-4 text-xs text-destructive-strong">
          <p>{t($ => $.messages[memberErrorKey(error)])}</p>
          <Button type="button" variant="outline" size="sm" onClick={() => void refetch()} disabled={isFetching}>{t($ => $.messages.retry)}</Button>
        </div>
      )}

      <div className="overflow-hidden rounded-xl border bg-surface" aria-busy={isFetching}>
        {!members && !error ? (
          <div role="status" className="space-y-4 p-5">
            <span className="sr-only">{t($ => $.messages.chargementDesMembres)}</span>
            {[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}
          </div>
        ) : members && filtered.length === 0 ? (
          <div className="flex flex-col items-center gap-3 px-5 py-12 text-center">
            <UsersIcon aria-hidden="true" className="size-8 text-muted-foreground" />
            <h2 className="font-display text-card-title">{needle ? t($ => $.messages.aucunMembreTrouve) : t($ => $.messages.aucunMembre)}</h2>
            <p className="text-xs text-muted-foreground">{needle ? t($ => $.messages.essayezUneAutreAdresseEmail) : t($ => $.messages.lesMembresDeLOrganisationApparaitrontIci)}</p>
          </div>
        ) : members ? (
          <Table>
            <caption className="sr-only">{t($ => $.messages.membresDeName, { name: organization.name })}</caption>
            <TableHeader>
              <TableRow>
                <TableHead scope="col">{t($ => $.messages.member)}</TableHead>
                <TableHead scope="col">{t($ => $.messages.role)}</TableHead>
                <TableHead scope="col" className="hidden min-[761px]:table-cell">{t($ => $.messages.addedAt)}</TableHead>
                {owner && <TableHead scope="col"><span className="sr-only">{t($ => $.messages.actions)}</span></TableHead>}
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((member) => (
                <TableRow key={member.email}>
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-2.5">
                      <span aria-hidden="true" className="hidden size-7.5 shrink-0 items-center justify-center rounded-full border bg-foreground-soft text-[10px] font-semibold text-muted-foreground min-[621px]:flex">
                        {Array.from(member.email).at(0)?.toLocaleUpperCase(locale)}
                      </span>
                      <div className="min-w-0 space-y-1">
                        <p className="max-w-[42vw] break-words font-semibold min-[621px]:max-w-none">{member.email}</p>
                        {member.email === user.email && <Badge variant="neutral">{t($ => $.messages.you)}</Badge>}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell><Badge variant="secondary">{member.access === "OWNER" ? t($ => $.messages.owner) : t($ => $.messages.member)}</Badge></TableCell>
                  <TableCell className="hidden text-muted-foreground min-[761px]:table-cell">
                    <time dateTime={member.created_at}>{dateFormat.format(new Date(member.created_at))}</time>
                  </TableCell>
                  {owner && <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      {member.email !== user.email && <EditMemberDialog user={user} organizationId={organization.id} member={member} disabled={!!error}
                        onUpdated={(updated) => showNotice({ key: updated.access === "OWNER" ? "memberBecameOwner" : "memberBecameMember", values: { email: updated.email } })}
                        onError={setMutationError} />}
                      {member.access === "MEMBER" && <RemoveMemberDialog user={user} organizationId={organization.id} member={member}
                        onRemoved={(email) => showNotice({ key: "emailAEteRetireDeLOrganisation", values: { email: email } })} onError={setMutationError} />}
                    </div>
                  </TableCell>}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ) : null}
        {members && <div className="flex flex-wrap items-center justify-between gap-2 border-t px-3.5 py-3 text-[10px] text-muted-foreground">
          <span>{t($ => $.messages.memberCount, { count: members.length })}</span>
          <span>{t($ => $.messages.seulsLesProprietairesPeuventAjouterOuRetirerDesMembres)}</span>
        </div>}
      </div>
    </section>
  );
}
