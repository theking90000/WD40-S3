import { useTranslation } from "react-i18next";
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
import { LanguageSwitcher } from "~/features/i18n/components/language-switcher";
import "./auth-screen.css";

export function AuthScreen({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  const features = [
    { icon: FolderIcon, title: t($ => $.messages.buckets), description: t($ => $.messages.organisezVosFichiers) },
    {
      icon: KeyRoundIcon,
      title: t($ => $.messages.credentials),
      description: t($ => $.messages.connectezVosApplications),
    },
    {
      icon: ShieldCheckIcon,
      title: t($ => $.messages.permissions),
      description: t($ => $.messages.controlezLesAcces),
    },
  ];
  const titleId = useId();
  return (
    <main className="auth-screen">
      <div className="auth-frame">
        <section className="auth-introduction" aria-label="WD40-S3">
          <p className="auth-brand">
            <DatabaseIcon aria-hidden="true" />
            <span>WD40-S3</span>
          </p>
          <h2>{t($ => $.messages.duStockagePourVosApplications)}</h2>
          <p className="auth-summary">
            {t($ => $.messages.unServiceCompatibleAmazonS3PourStockerEtRecuperer)}
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
          <div className="mb-4 flex justify-end"><LanguageSwitcher /></div>
          <h1 id={titleId}>{title}</h1>
          <p className="auth-description">{description}</p>
          {children && <div className="auth-actions">{children}</div>}
        </section>
      </div>
    </main>
  );
}

export function AuthPending() {
  const { t } = useTranslation();
  return (
    <AuthScreen
      title={t($ => $.messages.verificationDeLaSession)}
      description={t($ => $.messages.veuillezPatienterPourAccederALaConsole)}
    >
      <p role="status" className="text-sm text-muted-foreground">
        {t($ => $.messages.loading)}
      </p>
    </AuthScreen>
  );
}

export function AuthFailure({
  title,
  description,
  retry,
  busy = false,
}: {
  title?: string;
  description?: string;
  retry?: () => void;
  busy?: boolean;
}) {
  const { t } = useTranslation();
  const revalidator = useRevalidator();
  return (
    <AuthScreen title={title ?? t($ => $.messages.authentificationIndisponible)} description={description ?? t($ => $.messages.impossibleDeVerifierVotreSessionReessayezDansUnInstant)}>
      <div role="alert" className="flex flex-wrap gap-2">
        <Button
          onClick={retry ?? (() => revalidator.revalidate())}
          disabled={busy || revalidator.state !== "idle"}
        >
          {busy || revalidator.state !== "idle" ? t($ => $.messages.pending) : t($ => $.messages.retry)}
        </Button>
        <Button asChild variant="outline">
          <Link to="/login" replace>
            {t($ => $.messages.pageDeConnexion)}
          </Link>
        </Button>
      </div>
    </AuthScreen>
  );
}
