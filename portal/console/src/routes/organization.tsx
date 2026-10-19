import { Link } from "react-router";
import { useOrganization } from "~/features/organizations/router.ts";

export const handle = { title: "Vue d'ensemble" };

export default function OrganizationHome() {
  const organization = useOrganization();
  return (
    <section className="space-y-4">
      <h1 className="text-page-title">{organization?.name}</h1>
      <p className="text-muted-foreground">Console de gestion WD40.</p>
      {organization && <Link to={`/${organization.id}/credentials#s3-connection`} className="inline-block text-xs text-brand underline underline-offset-4 hover:text-foreground">Configurer un client S3</Link>}
    </section>
  );
}
