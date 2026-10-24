import { useId } from "react";
import type { ReactNode } from "react";
import { Link, useRevalidator } from "react-router";
import {
  DatabaseIcon,
  FolderIcon,
  KeyRoundIcon,
  ShieldCheckIcon,
} from "lucide-react";
import { Button } from "~/components/ui/button";
import "./auth-screen.css";

const features = [
  { icon: FolderIcon, title: "Buckets", description: "Organisez vos fichiers" },
  {
    icon: KeyRoundIcon,
    title: "Clés d'accès",
    description: "Connectez vos applications",
  },
  {
    icon: ShieldCheckIcon,
    title: "Permissions",
    description: "Contrôlez les accès",
  },
];

export function AuthScreen({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children?: ReactNode;
}) {
  const titleId = useId();
  return (
    <main className="auth-screen">
      <div className="auth-frame">
        <section className="auth-introduction" aria-label="WD40-S3">
          <p className="auth-brand">
            <DatabaseIcon aria-hidden="true" />
            <span>WD40-S3</span>
          </p>
          <h2>Du stockage pour vos applications.</h2>
          <p className="auth-summary">
            Un service compatible Amazon S3 pour stocker et récupérer vos
            fichiers depuis vos logiciels.
          </p>
          <ul className="auth-features">
            {features.map(({ icon: Icon, title, description }) => (
              <li key={title}>
                <Icon aria-hidden="true" />
                <div>
                  <p className="auth-feature-title">{title}</p>
                  <p className="auth-feature-description">{description}</p>
                </div>
              </li>
            ))}
          </ul>
        </section>
        <section className="auth-connection" aria-labelledby={titleId}>
          <h1 id={titleId}>{title}</h1>
          <p className="auth-description">{description}</p>
          {children && <div className="auth-actions">{children}</div>}
        </section>
      </div>
    </main>
  );
}

export function AuthPending() {
  return (
    <AuthScreen
      title="Vérification de la session"
      description="Veuillez patienter pour accéder à la console."
    >
      <p role="status" className="text-sm text-muted-foreground">
        Chargement…
      </p>
    </AuthScreen>
  );
}

export function AuthFailure({
  title = "Authentification indisponible",
  description = "Impossible de vérifier votre session. Réessayez dans un instant.",
  retry,
  busy = false,
}: {
  title?: string;
  description?: string;
  retry?: () => void;
  busy?: boolean;
}) {
  const revalidator = useRevalidator();
  return (
    <AuthScreen title={title} description={description}>
      <div role="alert" className="flex flex-wrap gap-2">
        <Button
          onClick={retry ?? (() => revalidator.revalidate())}
          disabled={busy || revalidator.state !== "idle"}
        >
          {busy || revalidator.state !== "idle" ? "En cours…" : "Réessayer"}
        </Button>
        <Button asChild variant="outline">
          <Link to="/login" replace>
            Page de connexion
          </Link>
        </Button>
      </div>
    </AuthScreen>
  );
}
