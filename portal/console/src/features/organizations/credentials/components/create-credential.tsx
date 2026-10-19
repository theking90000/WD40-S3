import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router";
import { ArrowLeftIcon, CheckIcon, LockKeyholeIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { CreatedCredential, CredentialInput } from "~/features/organizations/credentials/api.ts";
import { createOrganizationCredential } from "~/features/organizations/credentials/api.ts";
import { AuthError } from "~/lib/api.ts";
import { CredentialAccessForm, CredentialScope } from "./access-form";
import { CopyValue } from "~/components/copy-value";
import { S3Connection } from "~/components/s3-connection";
import { Button } from "~/components/ui/button";

export function CreateCredentialPage({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const [created, setCreated] = useState<CreatedCredential | null>(null);
  const active = useRef(false);
  const canReveal = useRef(false);
  const navigate = useNavigate();
  const mutation = useMutation({
    mutationFn: (input: CredentialInput) => {
      canReveal.current = true;
      // Only metadata is returned to TanStack. The secret stays in this page's state.
      return createOrganizationCredential(user, organization.id, input, (value) => {
        if (active.current && canReveal.current) setCreated(value);
      });
    },
    retry: false,
  });

  useEffect(() => {
    active.current = true;
    function clearSecret() {
      canReveal.current = false;
      setCreated(null);
    }
    window.addEventListener("pagehide", clearSecret);
    return () => {
      active.current = false;
      canReveal.current = false;
      window.removeEventListener("pagehide", clearSecret);
    };
  }, []);

  function clearCreated() {
    canReveal.current = false;
    setCreated(null);
    mutation.reset();
  }

  return (
    <section className="space-y-5">
      <Link to={`/${organization.id}/credentials`} className="inline-flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground">
        <ArrowLeftIcon aria-hidden="true" className="size-3.5" />Retour aux clés d'accès
      </Link>
      {created ? <CredentialCreated credential={created} organizationName={organization.name} onNew={clearCreated} onFinish={() => {
        clearCreated();
        void navigate(`/${organization.id}/credentials`, { replace: true });
      }} /> : (
        <>
          <div className="space-y-2">
            <p className="text-[10.5px] font-semibold text-muted-foreground">Stockage</p>
            <h1 className="text-page-title">Créer une clé d'accès</h1>
            <p className="text-muted-foreground">Choisissez les buckets accessibles dans {organization.name}.</p>
          </div>
          <div className="grid items-start gap-5 min-[961px]:grid-cols-[minmax(0,1fr)_280px]">
            <div className="space-y-5 rounded-xl border bg-surface p-5 min-[621px]:p-6">
              <div className="flex items-start gap-3 rounded-lg border bg-background p-3 text-xs">
                <LockKeyholeIcon aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                <p>Le secret sera affiché une seule fois. Enregistrez-le avant de quitter cette page.</p>
              </div>
              <CredentialAccessForm user={user} organizationId={organization.id} pending={mutation.isPending} error={mutation.error}
                submitLabel="Créer la clé" onEdit={() => { if (mutation.isError) mutation.reset(); }}
                onSubmit={(input) => mutation.mutate(input)} onCancel={() => void navigate(`/${organization.id}/credentials`)} />
              {mutation.isError && !(mutation.error instanceof AuthError && mutation.error.status < 500) && (
                <p role="alert" className="text-xs text-destructive-strong">La création a peut-être abouti. Consultez la liste des clés avant de réessayer ; un secret perdu ne peut pas être récupéré.</p>
              )}
              {mutation.isSuccess && <p role="status" className="text-xs text-muted-foreground">La clé a été créée, mais son secret n'est plus affiché. Consultez la liste pour supprimer cette clé si nécessaire.</p>}
            </div>
            <aside className="space-y-4">
              <div className="space-y-3 rounded-xl border bg-surface p-5">
                <p className="text-xs text-muted-foreground">Cette clé donne accès aux buckets sélectionnés.</p>
              </div>
              <div className="space-y-2 rounded-xl border bg-surface p-5 text-xs"><p className="text-muted-foreground">Organisation</p><p className="break-words font-semibold">{organization.name}</p></div>
            </aside>
          </div>
        </>
      )}
    </section>
  );
}

function CredentialCreated({ credential, organizationName, onNew, onFinish }: {
  credential: CreatedCredential;
  organizationName: string;
  onNew(): void;
  onFinish(): void;
}) {
  const heading = useRef<HTMLHeadingElement | null>(null);
  useEffect(() => { heading.current?.focus(); }, []);
  return (
    <>
      <div className="space-y-2">
        <p className="text-[10.5px] font-semibold text-muted-foreground">Clé d'accès</p>
        <h1 ref={heading} tabIndex={-1} className="text-page-title">Clé créée</h1>
        <p className="text-muted-foreground">Copiez le secret avant de quitter cette page. Il ne sera plus consultable ensuite.</p>
      </div>
      <div className="max-w-3xl space-y-5 rounded-xl border bg-surface p-5 min-[621px]:p-6">
        <p role="status" className="flex items-center gap-2 text-xs"><CheckIcon aria-hidden="true" className="size-4 shrink-0 text-success-strong" />La clé a été enregistrée dans {organizationName}.</p>
        <div className="space-y-2 rounded-lg border bg-background p-4">
          <div className="flex flex-wrap items-center justify-between gap-2"><h2 className="text-xs font-semibold">Secret d'accès</h2><CopyValue value={credential.secret} label="Copier le secret" /></div>
          <code className="block select-all break-all font-mono text-xs leading-relaxed">{credential.secret}</code>
        </div>
        <div className="space-y-2">
          <div className="flex flex-wrap items-center justify-between gap-2"><h2 className="text-xs font-semibold">Identifiant de la clé</h2><CopyValue value={credential.key_id} label="Copier l'identifiant" /></div>
          <code className="block select-all break-all font-mono text-xs">{credential.key_id}</code>
        </div>
        <S3Connection compact />
        <div className="space-y-2"><h2 className="text-xs font-semibold">Portée</h2><CredentialScope credential={credential} /></div>
        <p className="text-xs text-muted-foreground">Si vous perdez ce secret, supprimez la clé et créez-en une nouvelle.</p>
        <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-4">
          <Button type="button" variant="ghost" onClick={onNew}>Créer une autre clé</Button>
          <Button type="button" className="max-w-full whitespace-normal" onClick={onFinish}>J'ai enregistré le secret · Terminer</Button>
        </div>
      </div>
    </>
  );
}
