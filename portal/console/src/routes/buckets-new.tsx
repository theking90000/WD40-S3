import { Link, Navigate } from "react-router";
import { ArrowLeftIcon } from "lucide-react";
import { organizationOwnerMiddleware, useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { CreateBucketForm } from "~/features/organizations/buckets/components/create-bucket";

export const handle = { title: "Créer un bucket" };
export const clientMiddleware = [organizationOwnerMiddleware("buckets")];

export default function NewBucket() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  if (!organization) return null;
  if (organization.access !== "OWNER") return <Navigate to={`/${organization.id}/buckets`} replace />;

  return (
    <section className="space-y-5">
      <Link to={`/${organization.id}/buckets`} className="inline-flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground">
        <ArrowLeftIcon aria-hidden="true" className="size-3.5" />Retour aux buckets
      </Link>
      <div className="space-y-2">
        <p className="text-[10.5px] font-semibold text-muted-foreground">Stockage</p>
        <h1 className="text-page-title">Créer un bucket</h1>
        <p className="text-muted-foreground">Un nouveau bucket pour {organization.name}.</p>
      </div>
      <CreateBucketForm key={JSON.stringify(organizationBucketsQuery(user, organization.id).queryKey)} user={user} organization={organization} />
    </section>
  );
}
