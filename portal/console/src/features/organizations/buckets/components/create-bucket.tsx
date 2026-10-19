import { useEffect, useId, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { BucketAccess, CreateBucketInput } from "~/features/organizations/buckets/api.ts";
import { bucketErrorMessage, createOrganizationBucket, isValidBucketName } from "~/features/organizations/buckets/api.ts";
import { AuthError } from "~/lib/api.ts";
import { BucketAccessField } from "./access";
import { Button } from "~/components/ui/button";
import { Field, FieldDescription, FieldError, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";

export function CreateBucketForm({ user, organization }: { user: CurrentUser; organization: Organization }) {
  const [name, setName] = useState("");
  const [access, setAccess] = useState<BucketAccess>("PRIVATE");
  const [touched, setTouched] = useState(false);
  const nameInput = useRef<HTMLInputElement | null>(null);
  const nameId = useId();
  const hintId = useId();
  const errorId = useId();
  const rulesId = useId();
  const navigate = useNavigate();
  const mutation = useMutation({
    mutationFn: (input: CreateBucketInput) => createOrganizationBucket(user, organization.id, input),
    retry: false,
  });
  const validName = isValidBucketName(name);
  const serverNameError = mutation.error instanceof AuthError
    && (mutation.error.code === "invalid_bucket_name" || mutation.error.code === "bucket_already_exists");
  const nameError = touched && !validName
    ? "Saisissez un nom conforme aux règles de nommage sur cette page."
    : serverNameError ? bucketErrorMessage(mutation.error) : "";

  useEffect(() => {
    if (serverNameError) nameInput.current?.focus();
  }, [serverNameError, mutation.error]);

  return (
    <div className="grid items-start gap-5 min-[961px]:grid-cols-[minmax(0,1fr)_280px]">
      <form noValidate className="space-y-5 rounded-xl border bg-surface p-5 min-[621px]:p-6" onSubmit={(event) => {
        event.preventDefault();
        setTouched(true);
        if (!validName) nameInput.current?.focus();
        if (!mutation.isPending && validName && organization.access === "OWNER") {
          // Per-call callbacks stop when this form unmounts or the organization changes.
          mutation.mutate({ name, access }, { onSuccess: (bucket) => {
            void navigate(`/${bucket.organization_id}/buckets`, { replace: true });
          } });
        }
      }}>
        <Field data-invalid={!!nameError}>
          <FieldLabel htmlFor={nameId}>Nom du bucket</FieldLabel>
          <Input ref={nameInput} id={nameId} name="name" value={name} required autoComplete="off" autoCapitalize="none" autoCorrect="off" spellCheck={false}
            placeholder="prod-assets" className="font-mono" disabled={mutation.isPending}
            onBlur={() => setTouched(true)} onChange={(event) => {
              setName(event.target.value);
              if (mutation.isError) mutation.reset();
            }}
            aria-invalid={!!nameError} aria-describedby={`${hintId} ${rulesId}${nameError ? ` ${errorId}` : ""}`} />
          <FieldDescription id={hintId}>Unique entre toutes les organisations. Le nom restera inchangé après la création.</FieldDescription>
          {nameError && <FieldError id={errorId}>{nameError}</FieldError>}
        </Field>
        <BucketAccessField value={access} onChange={setAccess} disabled={mutation.isPending} />
        {mutation.isError && !nameError && <p role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{bucketErrorMessage(mutation.error)}</p>}
        <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-4">
          <Button type="button" variant="outline" disabled={mutation.isPending} onClick={() => void navigate(`/${organization.id}/buckets`)}>Annuler</Button>
          <Button type="submit" disabled={mutation.isPending || !name} aria-busy={mutation.isPending}>
            {mutation.isPending ? "Création en cours…" : "Créer le bucket"}
          </Button>
        </div>
      </form>
      <aside className="space-y-4">
        <div className="space-y-3 rounded-xl border bg-surface p-5">
          <h2 className="text-card-title">Nommer un bucket</h2>
          <ul id={rulesId} className="list-disc space-y-2 pl-4 text-xs leading-relaxed text-muted-foreground">
            <li>3 à 63 caractères : minuscules, chiffres, points et tirets.</li>
            <li>Commencer et terminer par une lettre ou un chiffre.</li>
            <li>Aucun point consécutif ni adresse IP.</li>
            <li>Aucun espace ni underscore. Le nom est conservé tel que saisi.</li>
          </ul>
          <details className="text-xs text-muted-foreground">
            <summary className="cursor-pointer font-semibold text-foreground">Préfixes et suffixes réservés</summary>
            <div className="mt-2 space-y-2 break-words leading-relaxed">
              <p>Préfixes : <code>xn--</code>, <code>sthree-</code>, <code>amzn-s3-demo-</code>.</p>
              <p>Suffixes : <code>-s3alias</code>, <code>--ol-s3</code>, <code>.mrap</code>, <code>--x-s3</code>, <code>--table-s3</code>, <code>-an</code>.</p>
            </div>
          </details>
        </div>
        <div className="space-y-2 rounded-xl border bg-surface p-5 text-xs">
          <p className="text-muted-foreground">Organisation</p>
          <p className="break-words font-semibold">{organization.name}</p>
        </div>
      </aside>
    </div>
  );
}
