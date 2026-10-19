import { useId, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { organizationErrorMessage, organizationQuery, updateOrganization } from "~/features/organizations/api.ts";
import { CopyValue } from "~/components/copy-value";
import { S3Connection } from "~/components/s3-connection";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Card } from "~/components/ui/card";
import { Field, FieldDescription, FieldError, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";
import { Skeleton } from "~/components/ui/skeleton";
import { Switch } from "~/components/ui/switch";
import { Textarea } from "~/components/ui/textarea";
import { DeleteOrganizationDialog } from "./delete-organization";

export function OrganizationSettingsPage({ user, organization, disabled }: {
  user: CurrentUser;
  organization: Organization;
  disabled?: boolean;
}) {
  const details = useQuery(organizationQuery(user, organization.id));
  // A refresh updates an untouched field without replacing an edit in progress.
  const [draft, setDraft] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const heading = useRef<HTMLHeadingElement | null>(null);
  const formId = useId();
  const nameId = useId();
  const nameHintId = useId();
  const nameErrorId = useId();
  const idId = useId();
  const storageRegionId = useId();
  const retentionId = useId();
  const ipsId = useId();
  const storageHintId = useId();
  const securityHintId = useId();
  const summaryId = useId();
  const current = details.data ?? organization;
  const name = draft ?? current.name;
  const trimmedName = name.trim();
  const tooLong = Array.from(trimmedName).length > 128;
  const owner = organization.access === "OWNER" && current.access === "OWNER";
  const canMutate = owner && !!details.data && !details.error && !disabled;
  const counts = details.data?.counts;
  const empty = counts?.buckets === 0 && counts?.credentials === 0;
  const numberFormat = new Intl.NumberFormat("fr-BE");
  const mutation = useMutation({
    mutationFn: (value: string) => updateOrganization(user, organization.id, value),
    retry: false,
    onSuccess: () => {
      setDraft(null);
      setNotice("Le nom de l'organisation a été enregistré.");
    },
  });
  const nameError = tooLong ? "Le nom doit contenir entre 1 et 128 caractères."
    : mutation.isError ? organizationErrorMessage(mutation.error) : "";
  const canSave = canMutate && !mutation.isPending && !!trimmedName && !tooLong && trimmedName !== current.name;

  return (
    <section className="space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">Organisation</p>
          <h1 ref={heading} tabIndex={-1} className="text-page-title">Paramètres</h1>
          <p className="text-xs text-muted-foreground">Gérez l'identité et les paramètres de votre organisation.</p>
        </div>
        {owner && <Button type="submit" form={formId} disabled={!canSave} aria-busy={mutation.isPending}
          className="w-full min-[621px]:w-auto">{mutation.isPending ? "Enregistrement en cours…" : "Enregistrer les modifications"}</Button>}
      </div>

      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{notice}</p>}
      {details.error && <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
        <p>{organizationErrorMessage(details.error)}{details.data && " Les données affichées proviennent du dernier chargement réussi."} Les modifications sont suspendues.</p>
        <Button type="button" variant="outline" size="sm" disabled={details.isFetching} onClick={() => void details.refetch()}>
          <RefreshCwIcon aria-hidden="true" />Réessayer
        </Button>
      </div>}

      <div className="grid items-start gap-3.5 min-[1161px]:grid-cols-[minmax(0,1fr)_288px]">
        <div className="min-w-0 space-y-3.5">
          <Card className="gap-0 p-5">
            <div className="space-y-1">
              <h2 className="text-card-title">Identité de l'organisation</h2>
              <p className="text-[10.5px] text-muted-foreground">Le nom est visible par tous les membres de l'organisation.</p>
            </div>
            <form id={formId} className="mt-4.5 grid gap-3.5 min-[761px]:grid-cols-2" onSubmit={(event) => {
              event.preventDefault();
              if (canSave) {
                setNotice("");
                mutation.mutate(name);
              }
            }}>
              <Field data-invalid={!!nameError}>
                <FieldLabel htmlFor={nameId}>Nom de l'organisation</FieldLabel>
                <Input id={nameId} name="name" value={name} autoComplete="organization" required readOnly={!owner}
                  disabled={mutation.isPending || !details.data || !!details.error || disabled}
                  aria-invalid={!!nameError} aria-describedby={`${nameHintId}${nameError ? ` ${nameErrorId}` : ""}`}
                  onChange={(event) => {
                    setDraft(event.target.value);
                    setNotice("");
                    if (mutation.isError) mutation.reset();
                  }} />
                <FieldDescription id={nameHintId}>{owner ? "Entre 1 et 128 caractères. Le nom doit être unique." : "Seuls les propriétaires peuvent modifier le nom."}</FieldDescription>
                {nameError && <FieldError id={nameErrorId}>{nameError}</FieldError>}
              </Field>
              <Field>
                <FieldLabel htmlFor={idId}>Identifiant de l'organisation</FieldLabel>
                <div className="flex min-w-0 items-start gap-1">
                  <Input id={idId} value={organization.id} readOnly className="min-w-0 flex-1 font-mono text-[10.5px]" />
                  <CopyValue value={organization.id} label="Copier l'identifiant de l'organisation" compact />
                </div>
                <FieldDescription>Identifiant permanent, non modifiable.</FieldDescription>
              </Field>
            </form>
          </Card>

          <Card className="gap-0 p-5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="space-y-1">
                <h2 className="text-card-title">Stockage par défaut</h2>
                <p id={storageHintId} className="text-[10.5px] text-muted-foreground">Ces réglages pour les nouveaux buckets ne sont pas encore disponibles.</p>
              </div>
              <Badge variant="secondary">Indisponible</Badge>
            </div>
            <fieldset disabled aria-describedby={storageHintId} className="mt-4.5 min-w-0">
              <legend className="sr-only">Stockage par défaut indisponible</legend>
              <div className="grid gap-3.5 min-[761px]:grid-cols-2">
                <Field data-disabled>
                  <FieldLabel htmlFor={storageRegionId}>Région des nouveaux buckets</FieldLabel>
                  <NativeSelect id={storageRegionId} disabled defaultValue=""><NativeSelectOption value="">Indisponible</NativeSelectOption></NativeSelect>
                </Field>
                <Field data-disabled>
                  <FieldLabel htmlFor={retentionId}>Rétention des versions</FieldLabel>
                  <NativeSelect id={retentionId} disabled defaultValue=""><NativeSelectOption value="">Indisponible</NativeSelectOption></NativeSelect>
                </Field>
              </div>
              <div className="mt-4 divide-y">
                <UnavailableSwitch label="Versioning activé par défaut" description="Conservation des versions précédentes des objets." />
                <UnavailableSwitch label="Chiffrement au repos" description="Chiffrement côté serveur pour les nouveaux objets." />
              </div>
            </fieldset>
          </Card>

          <Card className="gap-0 p-5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="space-y-1">
                <h2 className="text-card-title">Sécurité</h2>
                <p id={securityHintId} className="text-[10.5px] text-muted-foreground">Ces réglages de sécurité ne sont pas encore disponibles.</p>
              </div>
              <Badge variant="secondary">Indisponible</Badge>
            </div>
            <fieldset disabled aria-describedby={securityHintId} className="mt-3.5 min-w-0">
              <legend className="sr-only">Réglages de sécurité indisponibles</legend>
              <UnavailableSwitch label="Authentification multifacteur obligatoire" description="Exiger un second facteur pour accéder à l'organisation." />
              <Field data-disabled className="mt-4.5">
                <FieldLabel htmlFor={ipsId}>Adresses IP autorisées</FieldLabel>
                <Textarea id={ipsId} disabled placeholder="Indisponible" className="font-mono" />
              </Field>
            </fieldset>
          </Card>

          <Card className="gap-0 border-destructive/30 p-5">
            <div className="space-y-1">
              <h2 className="text-card-title text-destructive-strong">Zone sensible</h2>
              <p className="text-[10.5px] text-muted-foreground">La suppression de l'organisation et des appartenances de ses membres est définitive.</p>
            </div>
            <div className="mt-3.5 flex flex-col items-start gap-3 border-t border-destructive/20 pt-3.5 min-[621px]:flex-row min-[621px]:items-center min-[621px]:justify-between min-[621px]:gap-5">
              <div className="space-y-1 text-[10.5px] text-muted-foreground">
                <p>Seule une organisation sans bucket ni clé d'accès peut être supprimée.</p>
                {counts && !empty && <p>Cette organisation contient encore des ressources. La suppression des buckets n'est pas encore disponible.</p>}
                {!owner && <p>Seuls les propriétaires peuvent supprimer l'organisation.</p>}
              </div>
              {owner && <DeleteOrganizationDialog user={user} organization={current} empty={empty}
                disabled={!canMutate || mutation.isPending || !empty} restoreFocus={() => heading.current?.focus()} />}
            </div>
          </Card>
        </div>

        <aside aria-label="Récapitulatif de l'organisation" className="grid min-w-0 gap-3.5 min-[761px]:grid-cols-2 min-[1161px]:sticky min-[1161px]:top-21 min-[1161px]:grid-cols-1">
          <Card className="gap-0 p-4" aria-labelledby={summaryId} aria-busy={details.isPending}>
            <h2 id={summaryId} className="break-words text-[14.5px] font-bold">{current.name}</h2>
            {details.isPending && <span role="status" className="sr-only">Chargement du récapitulatif…</span>}
            <dl className="mt-4 grid grid-cols-2 gap-x-3 gap-y-3.5">
              <div className="flex flex-col gap-1 text-muted-foreground opacity-50">
                <dt className="text-[9px]">Stockage utilisé</dt>
                <dd className="order-first text-[10.5px]">Indisponible</dd>
              </div>
              {[
                { label: "Membres", count: counts?.members },
                { label: "Buckets", count: counts?.buckets },
                { label: "Clés d'accès", count: counts?.credentials },
              ].map(({ label, count }) => <div key={label} className="flex min-w-0 flex-col gap-1">
                <dt className="text-[9px] text-muted-foreground">{label}</dt>
                <dd className="order-first font-display text-[17px] leading-tight font-bold tabular-nums">
                  {count !== undefined ? numberFormat.format(count) : details.isPending ? <Skeleton className="h-5 w-10" /> : <span className="font-sans text-[10.5px] font-normal text-muted-foreground">Indisponible</span>}
                </dd>
              </div>)}
            </dl>
          </Card>
          <Card className="gap-0 p-4"><S3Connection compact /></Card>
        </aside>
      </div>
    </section>
  );
}

function UnavailableSwitch({ label, description }: { label: string; description: string }) {
  const id = useId();
  const hintId = useId();
  return (
    <div className="flex items-start justify-between gap-6 py-3.5 last:pb-0">
      <div className="min-w-0 space-y-0.5 text-muted-foreground opacity-55">
        <label htmlFor={id} className="block text-[11.5px] font-semibold">{label}</label>
        <p id={hintId} className="text-[10px]">{description} Indisponible.</p>
      </div>
      <Switch id={id} disabled checked={false} aria-describedby={hintId} className="mt-px" />
    </div>
  );
}
