import { useLocale } from "~/features/i18n/use-locale";
import { useTranslation } from "react-i18next";
import { useEffect, useId, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RefreshCwIcon, SearchIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Credential, CredentialInput, CredentialPermission } from "~/features/organizations/credentials/api.ts";
import { CREDENTIAL_PERMISSIONS, credentialErrorKey } from "~/features/organizations/credentials/api.ts";
import { bucketErrorKey, organizationBucketsQuery } from "~/features/organizations/buckets/api.ts";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Field, FieldDescription, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";
import { Skeleton } from "~/components/ui/skeleton";

function usePermissionLabels(): Record<CredentialPermission, string> {
  const { t } = useTranslation();
  return {
    READ: t($ => $.messages.lecture),
    WRITE: t($ => $.messages.ecriture),
    LIST: t($ => $.messages.liste),
    DELETE: t($ => $.messages.suppression),
  };
}

export function CredentialPermissions({ credential }: { credential: Credential }) {
  const { t } = useTranslation();
  const permissionLabels = usePermissionLabels();
  if (credential.permissions.length === 0) return <Badge variant="neutral">{t($ => $.messages.aucunePermission)}</Badge>;
  if (credential.permissions.length === CREDENTIAL_PERMISSIONS.length) return <Badge variant="neutral">{t($ => $.messages.toutesLesPermissions)}</Badge>;
  return <div className="flex flex-wrap gap-1">
    {CREDENTIAL_PERMISSIONS.filter((permission) => credential.permissions.includes(permission)).map((permission) => (
      <Badge key={permission} variant="neutral" title={permissionLabels[permission]}>{permission}</Badge>
    ))}
  </div>;
}

export function CredentialScope({ credential }: { credential: Credential }) {
  const { t } = useTranslation();
  if (credential.access_type === "ALL") return <Badge variant="neutral">{t($ => $.messages.tousLesBuckets)}</Badge>;
  if (credential.bucket_names.length === 0) return <Badge variant="warning">{t($ => $.messages.aucunBucketAutorise)}</Badge>;
  return (
    <details className="max-w-full text-xs">
      <summary className="cursor-pointer text-muted-foreground">{t($ => $.messages.selectedBuckets, { count: credential.bucket_names.length })}</summary>
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
  const { t } = useTranslation();
  const locale = useLocale();
  const permissionLabels = usePermissionLabels();
  const [input, setInput] = useState<CredentialInput>(initialInput ?? { access_type: "ALL", permissions: [...CREDENTIAL_PERMISSIONS] });
  const [search, setSearch] = useState("");
  const scopeId = useId();
  const hintId = useId();
  const selectionId = useId();
  const unavailableId = useId();
  const permissionsHintId = useId();
  const selectAll = useRef<HTMLInputElement | null>(null);
  const errorMessage = useRef<HTMLParagraphElement | null>(null);
  const { data: buckets, error: bucketsError, isFetching, refetch } = useQuery({
    ...organizationBucketsQuery(user, organizationId), enabled: input.access_type === "RESTRICTED",
  });
  const names = input.access_type === "RESTRICTED" ? input.bucket_names : [];
  const originalNames = initialInput?.access_type === "RESTRICTED" ? initialInput.bucket_names : [];
  const unchanged = initialInput !== undefined && initialInput.access_type === input.access_type
    && originalNames.length === names.length && originalNames.every((name) => names.includes(name))
    && initialInput.permissions.length === input.permissions.length
    && initialInput.permissions.every((permission) => input.permissions.includes(permission))
    && !error;
  const available = new Set(buckets?.map((bucket) => bucket.name));
  const unavailable = buckets ? names.filter((name) => !available.has(name)) : [];
  const valid = input.access_type === "ALL" || (buckets !== undefined && !bucketsError && unavailable.length === 0);
  const needle = search.trim().toLocaleLowerCase(locale);
  const filtered = buckets?.filter((bucket) => bucket.name.toLocaleLowerCase(locale).includes(needle)) ?? [];

  useEffect(() => {
    if (error) errorMessage.current?.focus();
  }, [error]);

  useEffect(() => {
    if (selectAll.current) selectAll.current.indeterminate = input.permissions.length > 0 && input.permissions.length < CREDENTIAL_PERMISSIONS.length;
  }, [input.permissions.length]);

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
        <FieldLabel htmlFor={scopeId}>{t($ => $.messages.porteeDesBuckets)}</FieldLabel>
        <NativeSelect id={scopeId} name="access_type" value={input.access_type} disabled={pending || disabled} aria-describedby={hintId}
          onChange={(event) => {
            if (event.target.value === "ALL") changeInput({ access_type: "ALL", permissions: input.permissions });
            if (event.target.value === "RESTRICTED") changeInput({ access_type: "RESTRICTED", bucket_names: [], permissions: input.permissions });
          }}>
          <NativeSelectOption value="ALL">{t($ => $.messages.tousLesBuckets)}</NativeSelectOption>
          <NativeSelectOption value="RESTRICTED">{t($ => $.messages.bucketsSelectionnes)}</NativeSelectOption>
        </NativeSelect>
        <FieldDescription id={hintId}>{input.access_type === "ALL"
          ? t($ => $.messages.tousLesBucketsDeLOrganisationYComprisCeux)
          : t($ => $.messages.uniquementLesBucketsCochesCiDessousUneSelectionVide)}</FieldDescription>
      </Field>
      {input.access_type === "RESTRICTED" && (
        <fieldset disabled={pending || disabled} className="min-w-0 space-y-3" aria-invalid={unavailable.length > 0}
          aria-describedby={`${selectionId}${unavailable.length > 0 ? ` ${unavailableId}` : ""}`}>
          <legend className="mb-3 text-xs font-semibold">{t($ => $.messages.bucketsAutorises)}</legend>
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-0 flex-1 basis-40">
              <SearchIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input type="search" value={search} onChange={(event) => setSearch(event.target.value)} className="pl-9"
                autoComplete="off" placeholder={t($ => $.messages.rechercherUnBucket2)} aria-label={t($ => $.messages.rechercherUnBucketAAutoriser)} disabled={!buckets} />
            </div>
            <Button type="button" variant="outline" size="sm" onClick={() => void refetch()} disabled={isFetching} aria-busy={isFetching}>
              <RefreshCwIcon aria-hidden="true" />{t($ => $.messages.refresh)}</Button>
          </div>
          {bucketsError && <p role="alert" className="text-xs text-destructive-strong">{t($ => $.messages[bucketErrorKey(bucketsError)])}{buckets && t($ => $.messages.laListeAfficheeProvientDuDernierChargementReussiActualisez)}</p>}
          {!buckets && !bucketsError ? (
            <div role="status" className="space-y-2 rounded-lg border p-3"><span className="sr-only">{t($ => $.messages.chargementDesBuckets)}</span><Skeleton className="h-7 w-full" /><Skeleton className="h-7 w-full" /></div>
          ) : buckets && (
            <div className="max-h-64 overflow-y-auto rounded-lg border bg-background">
              {filtered.length === 0 ? <p className="p-4 text-xs text-muted-foreground">{buckets.length === 0 ? t($ => $.messages.aucunBucketDisponibleDansCetteOrganisation) : t($ => $.messages.aucunBucketNeCorrespondALaRecherche)}</p>
                : filtered.map((bucket) => (
                  <label key={bucket.name} className="flex cursor-pointer items-center gap-3 border-b px-3 py-2.5 text-xs last:border-0 hover:bg-accent has-[:checked]:bg-accent has-[:focus-visible]:bg-accent has-disabled:cursor-not-allowed has-disabled:opacity-50">
                    <input type="checkbox" className="size-3.5 shrink-0 accent-foreground" checked={names.includes(bucket.name)} disabled={!!bucketsError}
                      onChange={(event) => {
                        const next = event.target.checked ? [...names, bucket.name].sort() : names.filter((name) => name !== bucket.name);
                        changeInput({ access_type: "RESTRICTED", bucket_names: next, permissions: input.permissions });
                      }} />
                    <span className="min-w-0 break-all font-mono">{bucket.name}</span>
                  </label>
                ))}
            </div>
          )}
          {unavailable.length > 0 && <div id={unavailableId} role="alert" className="space-y-2 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
            <p>{t($ => $.messages.cesBucketsNeSontPlusDisponiblesDansLOrganisation)}</p>
            <ul className="space-y-1 break-all font-mono">{unavailable.map((name) => <li key={name}>{name}</li>)}</ul>
            <Button type="button" variant="outline" size="sm" onClick={() => changeInput({ access_type: "RESTRICTED", bucket_names: names.filter((name) => available.has(name)), permissions: input.permissions })}>{t($ => $.messages.retirerDeLaSelection)}</Button>
          </div>}
          <p id={selectionId} className="text-[10.5px] text-muted-foreground" aria-live="polite">{names.length === 0 ? t($ => $.messages.aucunBucketAutorise2) : t($ => $.messages.selectedBucketsSentence, { count: names.length })}</p>
        </fieldset>
      )}
      <fieldset disabled={pending || disabled} className="min-w-0 space-y-3" aria-describedby={permissionsHintId}>
        <legend className="mb-3 text-xs font-semibold">{t($ => $.messages.permissions)}</legend>
        <label className="flex cursor-pointer items-center gap-3 rounded-lg border bg-background px-3 py-2.5 text-xs font-semibold has-[:checked]:border-input has-[:checked]:bg-accent has-[:focus-visible]:bg-accent has-disabled:cursor-not-allowed has-disabled:opacity-50">
          <input ref={selectAll} type="checkbox" className="size-3.5 shrink-0 accent-foreground"
            checked={input.permissions.length === CREDENTIAL_PERMISSIONS.length}
            aria-checked={input.permissions.length > 0 && input.permissions.length < CREDENTIAL_PERMISSIONS.length ? "mixed" : input.permissions.length === CREDENTIAL_PERMISSIONS.length}
            onChange={(event) => changeInput({ ...input, permissions: event.target.checked ? [...CREDENTIAL_PERMISSIONS] : [] })} />
          {t($ => $.messages.toutSelectionner)}</label>
        <div className="grid gap-2 min-[461px]:grid-cols-2">
          {CREDENTIAL_PERMISSIONS.map((permission) => <label key={permission}
            className="flex cursor-pointer items-center gap-3 rounded-lg border px-3 py-2.5 text-xs hover:bg-accent has-[:checked]:border-input has-[:checked]:bg-accent has-[:focus-visible]:bg-accent has-disabled:cursor-not-allowed has-disabled:opacity-50">
            <input type="checkbox" name="permissions" value={permission} className="size-3.5 shrink-0 accent-foreground"
              checked={input.permissions.includes(permission)} onChange={(event) => {
                const checked = event.target.checked;
                changeInput({ ...input, permissions: CREDENTIAL_PERMISSIONS.filter((item) => item === permission ? checked : input.permissions.includes(item)) });
              }} />
            <span className="min-w-0"><span className="block font-mono font-semibold">{permission}</span><span className="text-[10px] text-muted-foreground">{permissionLabels[permission]}</span></span>
          </label>)}
        </div>
        <p className="text-[10.5px] text-muted-foreground" aria-live="polite">{input.permissions.length === 0
          ? t($ => $.messages.aucunePermissionAccordeeParCetteCle)
          : t($ => $.messages.selectedPermissions, { count: input.permissions.length })}</p>
        <p id={permissionsHintId} className="text-[10.5px] text-muted-foreground">{t($ => $.messages.cesPermissionsSAppliquentAuxBucketsAutorisesLesAcces)}</p>
      </fieldset>
      {error && <p ref={errorMessage} tabIndex={-1} role="alert" className="rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">{t($ => $.messages[credentialErrorKey(error)])}</p>}
      <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-4">
        <Button type="button" variant="outline" disabled={pending} onClick={onCancel}>{t($ => $.messages.cancel)}</Button>
        <Button type="submit" disabled={pending || disabled || !valid || unchanged} aria-busy={pending}>{pending ? t($ => $.messages.enregistrement) : submitLabel}</Button>
      </div>
    </form>
  );
}
