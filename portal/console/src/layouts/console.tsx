import { useState } from "react";
import { ActivityIcon, LayoutGridIcon, UserRoundIcon } from "lucide-react";
import {
  isRouteErrorResponse, Outlet, useLoaderData, useMatches,
  useNavigate, useRouteError,
} from "react-router";
import { logout } from "~/auth.ts";
import { requireUserMiddleware, withUser } from "~/auth-router.ts";
import { AuthFailure } from "~/components/auth/auth-screen";
import { ConsoleShell } from "~/components/shell/console-shell";
import type { ShellNavigationGroup } from "~/components/shell/navigation";
import { Button } from "~/components/ui/button";

export const clientMiddleware = [requireUserMiddleware];
export const clientLoader = withUser();
export { AuthPending as HydrateFallback } from "~/components/auth/auth-screen";

const navigation: readonly ShellNavigationGroup[] = [
  {
    label: "Espace de travail",
    items: [
      { label: "Accueil", to: "/", icon: LayoutGridIcon, end: true },
      { label: "État du service", to: "/status", icon: ActivityIcon },
    ],
  },
];

function usePageTitle() {
  const matches = useMatches();
  let title = "Console";
  for (const { handle } of matches) {
    if (handle && typeof handle === "object" && "title" in handle
      && typeof handle.title === "string") {
      title = handle.title;
    }
  }
  return title;
}

export default function ConsoleLayout() {
  const { user } = useLoaderData<typeof clientLoader>();
  const title = usePageTitle();
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function disconnect() {
    setBusy(true);
    setError(null);
    try {
      await logout();
      await navigate("/login", { replace: true });
    } catch {
      setError("Impossible de vous déconnecter. Réessayez dans un instant.");
    } finally {
      setBusy(false);
    }
  }

  const account = (
    <div className="space-y-3">
      <div className="flex min-w-0 items-center gap-2.25">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-full border bg-foreground-soft text-muted-foreground">
          <UserRoundIcon aria-hidden="true" className="size-3.5" strokeWidth={1.6} />
        </span>
        <div className="min-w-0">
          <p className="truncate text-[11px] font-semibold">
            {user.name ?? user.email ?? user.subject}
          </p>
          {user.name && user.email && (
            <p className="truncate text-[9.5px] text-muted-foreground">{user.email}</p>
          )}
        </div>
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="w-full"
        onClick={disconnect}
        disabled={busy}
        aria-busy={busy}
      >
        {busy ? "En cours…" : "Se déconnecter"}
      </Button>
      {error && (
        <p role="alert" className="text-[10.5px] text-destructive-strong">{error}</p>
      )}
    </div>
  );

  return (
    <ConsoleShell title={title} navigation={navigation} account={account}>
      <Outlet />
    </ConsoleShell>
  );
}

export function ErrorBoundary() {
  const error = useRouteError();
  if (isRouteErrorResponse(error) && error.status === 404) {
    return <AuthFailure title="Page introuvable" description="Cette page n'existe pas." />;
  }
  return (
    <AuthFailure title="Console indisponible" description="Impossible d'accéder à la console. Réessayez dans un instant." />
  );
}
