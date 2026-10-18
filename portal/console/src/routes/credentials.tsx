import { Link } from "react-router";
import { PlusIcon } from "lucide-react";
import { useOrganization } from "~/features/organizations/router.ts";
import { Button } from "~/components/ui/button";

export const handle = { title: "Clés d'accès" };

export default function Credentials() {
  const organization = useOrganization();
  if (!organization) return null;

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">Stockage</p>
          <h1 className="text-page-title">Clés d'accès</h1>
          <p className="text-muted-foreground">Clés d'accès de {organization.name}.</p>
        </div>
        {organization.access === "OWNER" && (
          <Button asChild>
            <Link to={`/${organization.id}/credentials/new`}><PlusIcon aria-hidden="true" />Créer une clé</Link>
          </Button>
        )}
      </div>
    </section>
  );
}
