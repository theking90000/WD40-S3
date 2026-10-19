import { useEffect, useState } from "react";
import { ArrowRightIcon, Building2Icon, PlusIcon, RefreshCwIcon } from "lucide-react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { selectionLoader, useOrganizations } from "~/features/organizations/router.ts";
import { organizationErrorMessage } from "~/features/organizations/api.ts";
import { Account } from "~/features/auth/components/account";
import { AuthFailure, AuthPending } from "~/features/auth/components/auth-screen";
import { CreateOrganizationDialog } from "~/features/organizations/components/create-organization";
import { ConsoleShell } from "~/components/shell/console-shell";
import { Button } from "~/components/ui/button";
import { Badge } from "~/components/ui/badge";

export { selectionLoader as clientLoader };
export { AuthPending as HydrateFallback } from "~/features/auth/components/auth-screen";
export const handle = { title: "Vos organisations" };

export default function SelectOrganization() {
  const { user, data, error, isFetching, refetch } = useOrganizations();
  const [creating, setCreating] = useState(false);
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const unavailable = params.has("organization_unavailable");
  const onlyOrganization = data?.length === 1 ? data.at(0) : undefined;

  useEffect(() => {
    if (onlyOrganization && !unavailable) void navigate(`/${onlyOrganization.id}/`, { replace: true });
  }, [onlyOrganization, unavailable, navigate]);

  if (error) return <AuthFailure title="Organisations indisponibles" description={organizationErrorMessage(error)} retry={() => void refetch()} busy={isFetching} />;
  if (!data || (onlyOrganization && !unavailable)) return <AuthPending />;

  return (
    <ConsoleShell title="Vos organisations" navigation={[]} account={<Account user={user} />}>
      <section className="mx-auto max-w-3xl space-y-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="space-y-2">
            <h1 className="text-page-title">Vos organisations</h1>
            <p className="text-muted-foreground">Choisissez une organisation pour accéder à votre espace de travail.</p>
          </div>
          <Button type="button" variant="outline" size="sm" disabled={isFetching} aria-busy={isFetching} onClick={() => void refetch()}>
            <RefreshCwIcon aria-hidden="true" />{isFetching ? "Actualisation…" : "Actualiser"}
          </Button>
        </div>
        {unavailable && <p role="status" className="rounded-lg border bg-warning-soft p-3 text-warning-strong">Cette organisation n'existe pas ou vous n'y avez plus accès. Choisissez une autre organisation.</p>}
        {data.length === 0 ? (
          <div className="flex flex-col items-center gap-4 rounded-xl border bg-surface px-6 py-12 text-center">
            <span className="flex size-12 items-center justify-center rounded-xl bg-brand-soft text-brand"><Building2Icon aria-hidden="true" /></span>
            <h2 className="font-display text-card-title">Aucune organisation pour le moment</h2>
            <p className="max-w-md text-muted-foreground">Créez une organisation, ou demandez à son propriétaire de vous ajouter avec l'adresse {user.email}.</p>
            <p className="text-muted-foreground">Si vous venez d'être ajouté, actualisez la liste.</p>
            <Button type="button" onClick={() => setCreating(true)}><PlusIcon aria-hidden="true" />Créer une organisation</Button>
          </div>
        ) : (
          <>
            <div className="grid gap-3 min-[621px]:grid-cols-2">
              {data.map((organization) => (
                <Link key={organization.id} to={`/${organization.id}/`}
                  className="flex min-w-0 items-center gap-3 rounded-xl border bg-surface p-5 transition-colors hover:border-brand focus-visible:ring-2 focus-visible:ring-ring">
                  <Building2Icon aria-hidden="true" className="size-5 shrink-0 text-brand" />
                  <span className="min-w-0 flex-1 space-y-2">
                    <span className="block truncate font-semibold">{organization.name}</span>
                    <Badge variant="secondary">{organization.access === "OWNER" ? "Propriétaire" : "Membre"}</Badge>
                  </span>
                  <ArrowRightIcon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                </Link>
              ))}
            </div>
            <Button type="button" variant="outline" onClick={() => setCreating(true)}><PlusIcon aria-hidden="true" />Créer une organisation</Button>
          </>
        )}
      </section>
      <CreateOrganizationDialog user={user} open={creating} onOpenChange={setCreating} />
    </ConsoleShell>
  );
}
