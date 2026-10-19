import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RefreshCwIcon, SearchIcon, UsersIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { memberErrorMessage, organizationMembersQuery } from "~/features/organizations/members/api.ts";
import { AddMemberDialog } from "~/features/organizations/members/components/add-member";
import { EditMemberDialog } from "~/features/organizations/members/components/edit-member";
import { RemoveMemberDialog } from "~/features/organizations/members/components/remove-member";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { Skeleton } from "~/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "~/components/ui/table";

export const handle = { title: "Membres" };

export default function Members() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  return organization ? (
    <MembersPage key={JSON.stringify(organizationMembersQuery(user, organization.id).queryKey)} user={user} organization={organization} />
  ) : null;
}

function MembersPage({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const { data: members, error, isFetching, refetch } = useQuery(organizationMembersQuery(user, organization.id));
  const [search, setSearch] = useState("");
  const [notice, setNotice] = useState("");
  const [mutationError, setMutationError] = useState<Error | null>(null);
  const owner = organization.access === "OWNER";

  const needle = search.trim().toLocaleLowerCase("fr");
  const filtered = members?.filter(({ email }) => email.toLocaleLowerCase("fr").includes(needle)) ?? [];
  const dateFormat = new Intl.DateTimeFormat("fr-BE", { dateStyle: "medium" });

  function showNotice(message: string) {
    setMutationError(null);
    setNotice(message);
  }

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">Organisation</p>
          <h1 className="text-page-title">Membres</h1>
          <p className="text-muted-foreground">Contrôlez qui accède à l'organisation et aux ressources de stockage.</p>
          {members && <Badge variant="outline">{members.length} membre{members.length > 1 ? "s" : ""}</Badge>}
        </div>
        {owner && <AddMemberDialog user={user} organizationId={organization.id} disabled={!members || !!error}
          onAdded={(email) => showNotice(`${email} a été ajouté à l'organisation.`)} onError={setMutationError} />}
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-0 flex-1 sm:max-w-sm">
          <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)}
            placeholder="Rechercher un membre…" aria-label="Rechercher un membre" autoComplete="off" className="pl-9" disabled={!members} />
        </div>
        {members && <span className="text-[10.5px] text-muted-foreground" aria-live="polite">{filtered.length} résultat{filtered.length > 1 ? "s" : ""}</span>}
        <Button type="button" variant="outline" size="sm" className="ml-auto" onClick={() => void refetch()} disabled={isFetching} aria-busy={isFetching}>
          <RefreshCwIcon aria-hidden="true" />{isFetching ? "Actualisation…" : "Actualiser"}
        </Button>
      </div>

      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{notice}</p>}
      {mutationError && <p role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{memberErrorMessage(mutationError)}</p>}
      {error && (
        <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-4 text-xs text-destructive-strong">
          <p>{memberErrorMessage(error)}</p>
          <Button type="button" variant="outline" size="sm" onClick={() => void refetch()} disabled={isFetching}>Réessayer</Button>
        </div>
      )}

      <div className="overflow-hidden rounded-xl border bg-surface" aria-busy={isFetching}>
        {!members && !error ? (
          <div role="status" className="space-y-4 p-5">
            <span className="sr-only">Chargement des membres…</span>
            {[0, 1, 2].map((row) => <Skeleton key={row} className="h-10 w-full" />)}
          </div>
        ) : members && filtered.length === 0 ? (
          <div className="flex flex-col items-center gap-3 px-5 py-12 text-center">
            <UsersIcon aria-hidden="true" className="size-8 text-muted-foreground" />
            <h2 className="font-display text-card-title">{needle ? "Aucun membre trouvé" : "Aucun membre"}</h2>
            <p className="text-xs text-muted-foreground">{needle ? "Essayez une autre adresse email." : "Les membres de l'organisation apparaîtront ici."}</p>
          </div>
        ) : members ? (
          <Table>
            <caption className="sr-only">Membres de {organization.name}</caption>
            <TableHeader>
              <TableRow>
                <TableHead scope="col">Membre</TableHead>
                <TableHead scope="col">Rôle</TableHead>
                <TableHead scope="col" className="hidden min-[761px]:table-cell">Ajouté le</TableHead>
                {owner && <TableHead scope="col"><span className="sr-only">Actions</span></TableHead>}
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((member) => (
                <TableRow key={member.email}>
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-2.5">
                      <span aria-hidden="true" className="hidden size-7.5 shrink-0 items-center justify-center rounded-full border bg-foreground-soft text-[10px] font-semibold text-muted-foreground min-[621px]:flex">
                        {Array.from(member.email).at(0)?.toLocaleUpperCase("fr")}
                      </span>
                      <div className="min-w-0 space-y-1">
                        <p className="max-w-[42vw] break-words font-semibold min-[621px]:max-w-none">{member.email}</p>
                        {member.email === user.email && <Badge variant="neutral">Vous</Badge>}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell><Badge variant="secondary">{member.access === "OWNER" ? "Propriétaire" : "Membre"}</Badge></TableCell>
                  <TableCell className="hidden text-muted-foreground min-[761px]:table-cell">
                    <time dateTime={member.created_at}>{dateFormat.format(new Date(member.created_at))}</time>
                  </TableCell>
                  {owner && <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      {member.email !== user.email && <EditMemberDialog user={user} organizationId={organization.id} member={member} disabled={!!error}
                        onUpdated={(updated) => showNotice(`${updated.email} est maintenant ${updated.access === "OWNER" ? "propriétaire" : "membre"} de l'organisation.`)}
                        onError={setMutationError} />}
                      {member.access === "MEMBER" && <RemoveMemberDialog user={user} organizationId={organization.id} member={member}
                        onRemoved={(email) => showNotice(`${email} a été retiré de l'organisation.`)} onError={setMutationError} />}
                    </div>
                  </TableCell>}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ) : null}
        {members && <div className="flex flex-wrap items-center justify-between gap-2 border-t px-3.5 py-3 text-[10px] text-muted-foreground">
          <span>{members.length} membre{members.length > 1 ? "s" : ""}</span>
          <span>Seuls les propriétaires peuvent ajouter ou retirer des membres et modifier leur rôle.</span>
        </div>}
      </div>
    </section>
  );
}
