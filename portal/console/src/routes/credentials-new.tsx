import { routeTitle } from "~/features/i18n/route-titles";
import { Navigate } from "react-router";
import { organizationOwnerMiddleware, useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { organizationCredentialsQuery } from "~/features/organizations/credentials/api.ts";
import { CreateCredentialPage } from "~/features/organizations/credentials/components/create-credential";

export const handle = routeTitle("createCredential");
export const clientMiddleware = [organizationOwnerMiddleware("credentials")];

export default function NewCredential() {
  const organization = useOrganization();
  const { user } = useOrganizations();
  if (!organization) return null;
  if (organization.access !== "OWNER") return <Navigate to={`/${organization.id}/credentials`} replace />;

  return <CreateCredentialPage key={JSON.stringify(organizationCredentialsQuery(user, organization.id).queryKey)} user={user} organization={organization} />;
}
