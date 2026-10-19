import { organizationQuery } from "~/features/organizations/api.ts";
import { OrganizationSettingsPage } from "~/features/organizations/components/organization-settings";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";

export const handle = { title: "Paramètres" };

export default function Settings() {
  const organization = useOrganization();
  const { user, error } = useOrganizations();
  return organization ? (
    <OrganizationSettingsPage key={JSON.stringify(organizationQuery(user, organization.id).queryKey)}
      user={user} organization={organization} disabled={!!error} />
  ) : null;
}
