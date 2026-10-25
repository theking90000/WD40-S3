import { isRouteTitleHandle } from "~/features/i18n/route-titles";
import { useTranslation } from "react-i18next";
import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { DatabaseIcon, KeyRoundIcon, LayoutGridIcon, Settings2Icon, UsersIcon } from "lucide-react";
import { Outlet, useMatches, useNavigate } from "react-router";
import { organizationLoader, organizationMiddleware, useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { organizationErrorKey, organizationQuery } from "~/features/organizations/api.ts";
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
  const { t } = useTranslation();
  const { user, data, error, refetch, isFetching } = useOrganizations();
  const organization = useOrganization();
  const details = useQuery({
    ...organizationQuery(user, organization?.id ?? ""), enabled: !!organization,
  });
  const counts = details.isError ? undefined : details.data?.counts;
  const navigate = useNavigate();
  const matches = useMatches();
  let title = t($ => $.messages.console);
  for (const { handle } of matches) {
    if (isRouteTitleHandle(handle)) {
      title = t($ => $.routeTitles[handle.titleKey]);
    }
  }

  useEffect(() => {
    if (data && !organization) void navigate("/?organization_unavailable=1", { replace: true });
  }, [data, organization, navigate]);

  if (error) return <AuthFailure title={t($ => $.messages.organisationIndisponible)} description={t($ => $.messages[organizationErrorKey(error)])} retry={() => void refetch()} busy={isFetching} />;
  if (!organization) return <AuthPending />;

  const navigation: readonly ShellNavigationGroup[] = [
    {
      label: t($ => $.messages.workspace),
      items: [{ label: t($ => $.messages.overview), to: `/${organization.id}/`, icon: LayoutGridIcon, end: true }],
    },
    {
      label: t($ => $.messages.storage),
      items: [
        { label: t($ => $.messages.buckets), to: `/${organization.id}/buckets`, icon: DatabaseIcon, count: counts?.buckets },
        { label: t($ => $.messages.credentials), to: `/${organization.id}/credentials`, icon: KeyRoundIcon, count: counts?.credentials },
      ],
    },
    {
      label: t($ => $.messages.organization),
      items: [
        { label: t($ => $.messages.members), to: `/${organization.id}/members`, icon: UsersIcon, count: counts?.members },
        { label: t($ => $.messages.settings), to: `/${organization.id}/settings`, icon: Settings2Icon },
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
