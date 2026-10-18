import { useState } from "react";
import { ActivityIcon, LayoutGridIcon, UserRoundIcon } from "lucide-react";
import {
  isRouteErrorResponse, Link, Outlet, useLoaderData, useMatches,
  useRevalidator, useRouteError,
} from "react-router";
import type { LoaderFunctionArgs } from "react-router";
import { login, logout } from "~/auth.ts";
import { optionalUser } from "~/auth-router.ts";
import { ConsoleShell } from "~/components/shell/console-shell";
import type { ShellNavigationGroup } from "~/components/shell/navigation";
import { Button } from "~/components/ui/button";
import { Skeleton } from "~/components/ui/skeleton";

const navigation: readonly ShellNavigationGroup[] = [
  {
    label: "Espace de travail",
    items: [
      { label: "Accueil", to: "/", icon: LayoutGridIcon, end: true },
      { label: "État du service", to: "/status", icon: ActivityIcon },
    ],
  },
];

export async function clientLoader(args: LoaderFunctionArgs) {
  try {
    return { user: await optionalUser(args), error: null };
  } catch (error) {
    if (args.request.signal.aborted) throw error;
    return { user: null, error: "Authentification indisponible." };
  }
}
clientLoader.hydrate = true as const;

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

export function HydrateFallback() {
  const title = usePageTitle();
  return (
    <ConsoleShell
      title={title}
      navigation={navigation}
      account={<p className="text-[10.5px] text-muted-foreground">Chargement du compte…</p>}
    >
      <div role="status" className="space-y-4">
        <span className="sr-only">Chargement de la console…</span>
        <Skeleton aria-hidden="true" className="h-8 w-48" />
        <Skeleton aria-hidden="true" className="h-4 w-64 max-w-full" />
      </div>
    </ConsoleShell>
  );
}

export default function ConsoleLayout() {
  const { user, error: sessionError } = useLoaderData<typeof clientLoader>();
  const title = usePageTitle();
  const revalidator = useRevalidator();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function authenticate() {
    setBusy(true);
    setError(null);
    try {
      if (sessionError) {
        await revalidator.revalidate();
      } else if (user) {
        await logout();
        await revalidator.revalidate();
      } else {
        await login();
      }
    } catch (error) {
      setError(error instanceof Error ? error.message : "Authentification indisponible");
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
            {user ? user.name ?? user.email ?? user.subject : sessionError ? "Compte indisponible" : "Non connecté"}
          </p>
          {user?.name && user.email && (
            <p className="truncate text-[9.5px] text-muted-foreground">{user.email}</p>
          )}
        </div>
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="w-full"
        onClick={authenticate}
        disabled={busy || revalidator.state !== "idle"}
        aria-busy={busy}
      >
        {busy ? "En cours…" : sessionError ? "Réessayer" : user ? "Se déconnecter" : "Se connecter"}
      </Button>
      {(error || sessionError) && (
        <p role="alert" className="text-[10.5px] text-destructive-strong">{error || sessionError}</p>
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
  const title = isRouteErrorResponse(error) && error.status === 404
    ? "Page introuvable"
    : "Page indisponible";

  return (
    <ConsoleShell title={title} navigation={navigation}>
      <section className="space-y-4">
        <h1 className="text-page-title">{title}</h1>
        <p className="text-muted-foreground">Impossible d'afficher cette page pour le moment.</p>
        <Button asChild variant="outline">
          <Link to="/">Retour à l'accueil</Link>
        </Button>
      </section>
    </ConsoleShell>
  );
}
