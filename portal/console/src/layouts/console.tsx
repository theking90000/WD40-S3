import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { DatabaseIcon, KeyRoundIcon, LayoutGridIcon, Settings2Icon, UsersIcon } from "lucide-react";
import { Outlet, useMatches, useNavigate } from "react-router";
import { organizationLoader, organizationMiddleware, useOrganization, useOrganizations } from "~/organization-router.ts";
import { organizationErrorMessage } from "~/organizations.ts";
import { organizationMembersQuery } from "~/organization-members.ts";
import { AuthFailure, AuthPending } from "~/components/auth/auth-screen";
import { Account } from "~/components/auth/account";
import { OrganizationSwitcher } from "~/components/organizations/organization-switcher";
import { ConsoleShell } from "~/components/shell/console-shell";
import type { ShellNavigationGroup } from "~/components/shell/navigation";

export const clientMiddleware = [organizationMiddleware];
export { organizationLoader as clientLoader };
export { AuthPending as HydrateFallback } from "~/components/auth/auth-screen";
export { ErrorBoundary } from "./session";

export default function ConsoleLayout() {
  const { user, data, error, refetch, isFetching } = useOrganizations();
  const organization = useOrganization();
  // Observe an already loaded count without fetching members for other pages.
  const { data: members } = useQuery({
    ...organizationMembersQuery(user, organization?.id ?? ""), enabled: false,
  });
  const navigate = useNavigate();
  const matches = useMatches();
  let title = "Console";
  for (const { handle } of matches) {
    if (handle && typeof handle === "object" && "title" in handle && typeof handle.title === "string") {
      title = handle.title;
    }
  }

  useEffect(() => {
    if (data && !organization) void navigate("/?organization_unavailable=1", { replace: true });
  }, [data, organization, navigate]);

  if (error) return <AuthFailure title="Organisation indisponible" description={organizationErrorMessage(error)} retry={() => void refetch()} busy={isFetching} />;
  if (!organization) return <AuthPending />;

  const navigation: readonly ShellNavigationGroup[] = [
    {
      label: "Espace de travail",
      items: [{ label: "Vue d'ensemble", to: `/${organization.id}/`, icon: LayoutGridIcon, end: true }],
    },
    {
      label: "Stockage",
      items: [
        { label: "Buckets", icon: DatabaseIcon, disabled: true },
        { label: "Clés d'accès", icon: KeyRoundIcon, disabled: true },
      ],
    },
    {
      label: "Organisation",
      items: [
        { label: "Membres", to: `/${organization.id}/members`, icon: UsersIcon, count: members?.length },
        { label: "Paramètres", icon: Settings2Icon, disabled: true },
      ],
    },
  ];

  return (
    <ConsoleShell title={title} navigation={navigation} organization={<OrganizationSwitcher />}
      organizationId={organization.id} organizationName={organization.name} account={<Account user={user} />}>
      <Outlet />
    </ConsoleShell>
  );
}
