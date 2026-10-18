import { Link, Navigate } from "react-router";
import { ArrowLeftIcon } from "lucide-react";
import { useOrganization } from "~/features/organizations/router.ts";

export const handle = { title: "Créer une clé d'accès" };

export default function NewCredential() {
  const organization = useOrganization();
  if (!organization) return null;
  if (organization.access !== "OWNER") return <Navigate to={`/${organization.id}/credentials`} replace />;

  return (
    <section className="space-y-5">
      <Link to={`/${organization.id}/credentials`} className="inline-flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground">
        <ArrowLeftIcon aria-hidden="true" className="size-3.5" />Retour aux clés d'accès
      </Link>
      <div className="space-y-2">
        <p className="text-[10.5px] font-semibold text-muted-foreground">Stockage</p>
        <h1 className="text-page-title">Créer une clé d'accès</h1>
        <p className="text-muted-foreground">Une nouvelle clé pour {organization.name}.</p>
      </div>
    </section>
  );
}
