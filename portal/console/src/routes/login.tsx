import { useEffect, useState } from "react";
import { useLoaderData } from "react-router";
import { login } from "~/features/auth/session.ts";
import { loginLoader } from "~/features/auth/router.ts";
import { AuthScreen } from "~/features/auth/components/auth-screen";
import { Button } from "~/components/ui/button";

export { loginLoader as clientLoader };
export { AuthPending as HydrateFallback, AuthFailure as ErrorBoundary } from "~/features/auth/components/auth-screen";

export default function Login() {
  const { returnTo } = useLoaderData<typeof loginLoader>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    function restore(event: PageTransitionEvent) {
      // A history restore preserves the state from before the OIDC redirect.
      if (event.persisted) setBusy(false);
    }
    window.addEventListener("pageshow", restore);
    return () => window.removeEventListener("pageshow", restore);
  }, []);

  async function connect() {
    setBusy(true);
    setError(null);
    try {
      await login(returnTo);
    } catch {
      setError("Impossible de démarrer la connexion. Réessayez dans un instant.");
      setBusy(false);
    }
  }

  return (
    <AuthScreen title="Connexion à la console" description="Connectez-vous avec votre compte pour accéder à WD40-S3.">
      <Button className="w-full" onClick={connect} disabled={busy} aria-busy={busy}>
        {busy ? "Connexion en cours…" : "Se connecter"}
      </Button>
      {error && <p role="alert" className="text-sm text-destructive-strong">{error}</p>}
    </AuthScreen>
  );
}
