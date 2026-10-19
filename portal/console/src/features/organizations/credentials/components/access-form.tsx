import { useEffect, useId, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RefreshCwIcon, SearchIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Credential, CredentialInput } from "~/features/organizations/credentials/api.ts";
import { credentialErrorMessage } from "~/features/organizations/credentials/api.ts";
import { bucketErrorMessage, organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Field, FieldDescription, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";
import { Skeleton } from "~/components/ui/skeleton";

export function CredentialScope({ credential }: { credential: Credential }) {
  if (credential.access_type === "ALL") return <Badge variant="neutral">Tous les buckets</Badge>;
  if (credential.bucket_names.length === 0) return <Badge variant="warning">Aucun bucket autorisé</Badge>;
  return (
    <details className="max-w-full text-xs">
      <summary className="cursor-pointer text-muted-foreground">{credential.bucket_names.length} bucket{credential.bucket_names.length > 1 ? "s" : ""} sélectionné{credential.bucket_names.length > 1 ? "s" : ""}</summary>
      <ul className="mt-2 space-y-1 break-all font-mono text-[10.5px]">
        {credential.bucket_names.map((name) => <li key={name}>{name}</li>)}
      </ul>
    </details>
  );
}

export function CredentialAccessForm({ user, organizationId, initialInput, pending, disabled, error, onEdit, onSubmit, onCancel, submitLabel }: {
  user: CurrentUser;
  organizationId: string;
  initialInput?: CredentialInput;
  pending: boolean;
  disabled?: boolean;
  error: Error | null;
  onEdit(): void;
  onSubmit(input: CredentialInput): void;
  onCancel(): void;
  submitLabel: string;
}) {
  const [input, setInput] = useState<CredentialInput>(initialInput ?? { access_type: "ALL" });
  const [search, setSearch] = useState("");
  const scopeId = useId();
  const hintId = useId();
  const selectionId = useId();
  const unavailableId = useId();
  const errorMessage = useRef<HTMLParagraphElement | null>(null);
  const { data: buckets, error: bucketsError, isFetching, refetch } = useQuery({
    ...organizationBucketsQuery(user, organizationId), enabled: input.access_type === "RESTRICTED",
  });
  const names = input.access_type === "RESTRICTED" ? input.bucket_names : [];
  const originalNames = initialInput?.access_type === "RESTRICTED" ? initialInput.bucket_names : [];
  const unchanged = initialInput?.access_type === input.access_type
    && originalNames.length === names.length && originalNames.every((name, index) => name === names[index]);
  const available = new Set(buckets?.map((bucket) => bucket.name));
  const unavailable = buckets ? names.filter((name) => !available.has(name)) : [];
  const valid = input.access_type === "ALL" || (buckets !== undefined && !bucketsError && unavailable.length === 0);
  const needle = search.trim().toLocaleLowerCase("fr");
  const filtered = buckets?.filter((bucket) => bucket.name.toLocaleLowerCase("fr").includes(needle)) ?? [];

  useEffect(() => {
    if (error) errorMessage.current?.focus();
  }, [error]);

  function changeInput(next: CredentialInput) {
    setInput(next);
    onEdit();
  }

  return (
    <form className="space-y-5" onSubmit={(event) => {
      event.preventDefault();
      if (!pending && !disabled && valid && !unchanged) onSubmit(input);
    }}>
      <Field>
        <FieldLabel htmlFor={scopeId}>Portée</FieldLabel>
        <NativeSelect id={scopeId} name="access_type" value={input.access_type} disabled={pending || disabled} aria-describedby={hintId}
          onChange={(event) => {
            if (event.target.value === "ALL") changeInput({ access_type: "ALL" });
            if (event.target.value === "RESTRICTED") changeInput({ access_type: "RESTRICTED", bucket_names: [] });
          }}>
          <NativeSelectOption value="ALL">Tous les buckets</NativeSelectOption>
          <NativeSelectOption value="RESTRICTED">Buckets sélectionnés</NativeSelectOption>
        </NativeSelect>
        <FieldDescription id={hintId}>{input.access_type === "ALL"
          ? "Tous les buckets de l'organisation, y compris ceux créés ensuite."
          : "Uniquement les buckets cochés ci-dessous. Une sélection vide n'autorise aucun bucket."}</FieldDescription>
      </Field>
      {input.access_type === "RESTRICTED" && (
        <fieldset disabled={pending || disabled} className="min-w-0 space-y-3" aria-invalid={unavailable.length > 0}
          aria-describedby={`${selectionId}${unavailable.length > 0 ? ` ${unavailableId}` : ""}`}>
          <legend className="mb-3 text-xs font-semibold">Buckets autorisés</legend>
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-0 flex-1 basis-40">
              <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)} className="pl-9"
                autoComplete="off" placeholder="Rechercher un bucket…" aria-label="Rechercher un bucket à autoriser" disabled={!buckets} />
            </div>
            <Button type="button" variant="outline" size="sm" onClick={() => void refetch()} disabled={isFetching} aria-busy={isFetching}>
              <RefreshCwIcon aria-hidden="true" />Actualiser
            </Button>
          </div>
          {bucketsError && <p role="alert" className="text-xs text-destructive-strong">{bucketErrorMessage(bucketsError)}{buckets && " La liste affichée provient du dernier chargement réussi. Actualisez-la avant d'enregistrer la sélection."}</p>}
          {!buckets && !bucketsError ? (
            <div role="status" className="space-y-2 rounded-lg border p-3"><span className="sr-only">Chargement des buckets…</span><Skeleton className="h-7 w-full" /><Skeleton className="h-7 w-full" /></div>
          ) : buckets && (
            <div className="max-h-64 overflow-y-auto rounded-lg border bg-background">
              {filtered.length === 0 ? <p className="p-4 text-xs text-muted-foreground">{buckets.length === 0 ? "Aucun bucket disponible dans cette organisation." : "Aucun bucket ne correspond à la recherche."}</p>
                : filtered.map((bucket) => (
                  <label key={bucket.name} className="flex cursor-pointer items-center gap-3 border-b px-3 py-2.5 text-xs last:border-0 hover:bg-accent has-disabled:cursor-not-allowed has-disabled:opacity-50">
                    <input type="checkbox" className="size-3.5 shrink-0 accent-foreground" checked={names.includes(bucket.name)} disabled={!!bucketsError}
                      onChange={(event) => {
                        const next = event.target.checked ? [...names, bucket.name].sort() : names.filter((name) => name !== bucket.name);
                        changeInput({ access_type: "RESTRICTED", bucket_names: next });
                      }} />
                    <span className="min-w-0 break-all font-mono">{bucket.name}</span>
                  </label>
                ))}
            </div>
          )}
          {unavailable.length > 0 && <div id={unavailableId} role="alert" className="space-y-2 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
            <p>Ces buckets ne sont plus disponibles dans l'organisation :</p>
            <ul className="space-y-1 break-all font-mono">{unavailable.map((name) => <li key={name}>{name}</li>)}</ul>
            <Button type="button" variant="outline" size="sm" onClick={() => changeInput({ access_type: "RESTRICTED", bucket_names: names.filter((name) => available.has(name)) })}>Retirer de la sélection</Button>
          </div>}
          <p id={selectionId} className="text-[10.5px] text-muted-foreground" aria-live="polite">{names.length === 0 ? "Aucun bucket autorisé." : `${names.length} bucket${names.length > 1 ? "s" : ""} sélectionné${names.length > 1 ? "s" : ""}.`}</p>
        </fieldset>
      )}
      {error && <p ref={errorMessage} tabIndex={-1} role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{credentialErrorMessage(error)}</p>}
      <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-4">
        <Button type="button" variant="outline" disabled={pending} onClick={onCancel}>Annuler</Button>
        <Button type="submit" disabled={pending || disabled || !valid || unchanged} aria-busy={pending}>{pending ? "Enregistrement…" : submitLabel}</Button>
      </div>
    </form>
  );
}
