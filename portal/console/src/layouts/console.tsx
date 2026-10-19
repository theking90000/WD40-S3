import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { DatabaseIcon, KeyRoundIcon, LayoutGridIcon, Settings2Icon, UsersIcon } from "lucide-react";
import { Outlet, useMatches, useNavigate } from "react-router";
import { organizationLoader, organizationMiddleware, useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { organizationErrorMessage, organizationQuery } from "~/features/organizations/api.ts";
import { AuthFailure, AuthPending } from "~/features/auth/components/auth-screen";
import { Account } from "~/features/auth/components/account";
import { OrganizationSwitcher } from "~/features/organizations/components/organization-switcher";
import { ConsoleShell } from "~/components/shell/console-shell";
import type { ShellNavigationGroup } from "~/components/shell/navigation";

export const clientMiddleware = [organizationMiddleware];
export { organizationLoader as clientLoader };
export { AuthPending as HydrateFallback } from "~/features/auth/components/auth-screen";
export { ErrorBoundary } from "./session";

export default function ConsoleLayout() {
  const { user, data, error, refetch, isFetching } = useOrganizations();
  const organization = useOrganization();
  const details = useQuery({
    ...organizationQuery(user, organization?.id ?? ""), enabled: !!organization,
  });
  const counts = details.isError ? undefined : details.data?.counts;
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
        { label: "Buckets", to: `/${organization.id}/buckets`, icon: DatabaseIcon, count: counts?.buckets },
        { label: "Clés d'accès", to: `/${organization.id}/credentials`, icon: KeyRoundIcon, count: counts?.credentials },
      ],
    },
    {
      label: "Organisation",
      items: [
        { label: "Membres", to: `/${organization.id}/members`, icon: UsersIcon, count: counts?.members },
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
