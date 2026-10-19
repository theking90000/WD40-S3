import { useId } from "react";
import type { ReactNode } from "react";
import { Link, useRevalidator } from "react-router";
import { Button } from "~/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader } from "~/components/ui/card";

export function AuthScreen({ title, description, children }: {
  title: string;
  description: string;
  children?: ReactNode;
}) {
  const titleId = useId();
  return (
    <main className="m-0 flex min-h-dvh max-w-none items-center justify-center p-6">
      <Card className="w-full max-w-sm" aria-labelledby={titleId}>
        <CardHeader className="gap-3">
          <p className="font-display text-card-title">WD40-S3</p>
          <h1 id={titleId} className="text-page-title">{title}</h1>
          <CardDescription className="text-sm">{description}</CardDescription>
        </CardHeader>
        {children && <CardContent className="space-y-4">{children}</CardContent>}
      </Card>
    </main>
  );
}

export function AuthPending() {
  return (
    <AuthScreen title="Vérification de la session" description="Veuillez patienter pour accéder à la console.">
      <p role="status" className="text-sm text-muted-foreground">Chargement…</p>
    </AuthScreen>
  );
}

export function AuthFailure({ title = "Authentification indisponible", description = "Impossible de vérifier votre session. Réessayez dans un instant.", retry, busy = false }: {
  title?: string;
  description?: string;
  retry?: () => void;
  busy?: boolean;
}) {
  const revalidator = useRevalidator();
  return (
    <AuthScreen title={title} description={description}>
      <div role="alert" className="flex flex-wrap gap-2">
        <Button onClick={retry ?? (() => revalidator.revalidate())} disabled={busy || revalidator.state !== "idle"}>
          {busy || revalidator.state !== "idle" ? "En cours…" : "Réessayer"}
        </Button>
        <Button asChild variant="outline">
          <Link to="/login" replace>Page de connexion</Link>
        </Button>
      </div>
    </AuthScreen>
  );
}
