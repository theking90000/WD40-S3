import type { Message } from "~/features/i18n/message";
import { useTranslation } from "react-i18next";
import { useId, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { organizationErrorKey, organizationQuery, updateOrganization } from "~/features/organizations/api.ts";
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
  const { t } = useTranslation();
  const details = useQuery(organizationQuery(user, organization.id));
  // A refresh updates an untouched field without replacing an edit in progress.
  const [draft, setDraft] = useState<string | null>(null);
  const [notice, setNotice] = useState<Message | null>(null);
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
      setNotice({ key: "leNomDeLOrganisationAEteEnregistre" });
    },
  });
  const nameError = tooLong ? t($ => $.messages.leNomDoitContenirEntre1Et128Caracteres)
    : mutation.isError ? t($ => $.messages[organizationErrorKey(mutation.error)]) : "";
  const canSave = canMutate && !mutation.isPending && !!trimmedName && !tooLong && trimmedName !== current.name;

  return (
    <section className="space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 space-y-2">
          <p className="text-[10.5px] font-semibold text-muted-foreground">{t($ => $.messages.organization)}</p>
          <h1 ref={heading} tabIndex={-1} className="text-page-title">{t($ => $.messages.settings)}</h1>
          <p className="text-xs text-muted-foreground">{t($ => $.messages.gerezLIdentiteEtLesParametresDeVotreOrganisation)}</p>
        </div>
        {owner && <Button type="submit" form={formId} disabled={!canSave} aria-busy={mutation.isPending}
          className="w-full min-[621px]:w-auto">{mutation.isPending ? t($ => $.messages.enregistrementEnCours) : t($ => $.messages.enregistrerLesModifications)}</Button>}
      </div>

      {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{t($ => $.messages[notice.key], notice.values)}</p>}
      {details.error && <div role="alert" className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-destructive-soft p-3 text-xs text-destructive-strong">
        <p>{t($ => $.messages[organizationErrorKey(details.error)])}{details.data && t($ => $.messages.lesDonneesAfficheesProviennentDuDernierChargementReussi)} {t($ => $.messages.lesModificationsSontSuspendues)}</p>
        <Button type="button" variant="outline" size="sm" disabled={details.isFetching} onClick={() => void details.refetch()}>
          <RefreshCwIcon aria-hidden="true" />{t($ => $.messages.retry)}</Button>
      </div>}

      <div className="grid items-start gap-3.5 min-[1161px]:grid-cols-[minmax(0,1fr)_288px]">
        <div className="min-w-0 space-y-3.5">
          <Card className="gap-0 p-5">
            <div className="space-y-1">
              <h2 className="text-card-title">{t($ => $.messages.identiteDeLOrganisation)}</h2>
              <p className="text-[10.5px] text-muted-foreground">{t($ => $.messages.leNomEstVisibleParTousLesMembresDe)}</p>
            </div>
            <form id={formId} className="mt-4.5 grid gap-3.5 min-[761px]:grid-cols-2" onSubmit={(event) => {
              event.preventDefault();
              if (canSave) {
                setNotice(null);
                mutation.mutate(name);
              }
            }}>
              <Field data-invalid={!!nameError}>
                <FieldLabel htmlFor={nameId}>{t($ => $.messages.nomDeLOrganisation)}</FieldLabel>
                <Input id={nameId} name="name" value={name} autoComplete="organization" required readOnly={!owner}
                  disabled={mutation.isPending || !details.data || !!details.error || disabled}
                  aria-invalid={!!nameError} aria-describedby={`${nameHintId}${nameError ? ` ${nameErrorId}` : ""}`}
                  onChange={(event) => {
                    setDraft(event.target.value);
                    setNotice(null);
                    if (mutation.isError) mutation.reset();
                  }} />
                <FieldDescription id={nameHintId}>{owner ? t($ => $.messages.entre1Et128CaracteresLeNomDoitEtre) : t($ => $.messages.seulsLesProprietairesPeuventModifierLeNom)}</FieldDescription>
                {nameError && <FieldError id={nameErrorId}>{nameError}</FieldError>}
              </Field>
              <Field>
                <FieldLabel htmlFor={idId}>{t($ => $.messages.identifiantDeLOrganisation)}</FieldLabel>
                <div className="flex min-w-0 items-start gap-1">
                  <Input id={idId} value={organization.id} readOnly className="min-w-0 flex-1 font-mono text-[10.5px]" />
                  <CopyValue value={organization.id} label={t($ => $.messages.copierLIdentifiantDeLOrganisation)} compact />
                </div>
                <FieldDescription>{t($ => $.messages.identifiantPermanentNonModifiable)}</FieldDescription>
              </Field>
            </form>
          </Card>

          <Card className="gap-0 p-5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="space-y-1">
                <h2 className="text-card-title">{t($ => $.messages.stockageParDefaut)}</h2>
                <p id={storageHintId} className="text-[10.5px] text-muted-foreground">{t($ => $.messages.cesReglagesPourLesNouveauxBucketsNeSontPas)}</p>
              </div>
              <Badge variant="secondary">{t($ => $.messages.unavailable)}</Badge>
            </div>
            <fieldset disabled aria-describedby={storageHintId} className="mt-4.5 min-w-0">
              <legend className="sr-only">{t($ => $.messages.stockageParDefautIndisponible)}</legend>
              <div className="grid gap-3.5 min-[761px]:grid-cols-2">
                <Field data-disabled>
                  <FieldLabel htmlFor={storageRegionId}>{t($ => $.messages.regionDesNouveauxBuckets)}</FieldLabel>
                  <NativeSelect id={storageRegionId} disabled defaultValue=""><NativeSelectOption value="">{t($ => $.messages.unavailable)}</NativeSelectOption></NativeSelect>
                </Field>
                <Field data-disabled>
                  <FieldLabel htmlFor={retentionId}>{t($ => $.messages.retentionDesVersions)}</FieldLabel>
                  <NativeSelect id={retentionId} disabled defaultValue=""><NativeSelectOption value="">{t($ => $.messages.unavailable)}</NativeSelectOption></NativeSelect>
                </Field>
              </div>
              <div className="mt-4 divide-y">
                <UnavailableSwitch label={t($ => $.messages.versioningActiveParDefaut)} description={t($ => $.messages.conservationDesVersionsPrecedentesDesObjets)} />
                <UnavailableSwitch label={t($ => $.messages.chiffrementAuRepos)} description={t($ => $.messages.chiffrementCoteServeurPourLesNouveauxObjets)} />
              </div>
            </fieldset>
          </Card>

          <Card className="gap-0 p-5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="space-y-1">
                <h2 className="text-card-title">{t($ => $.messages.securite)}</h2>
                <p id={securityHintId} className="text-[10.5px] text-muted-foreground">{t($ => $.messages.cesReglagesDeSecuriteNeSontPasEncoreDisponibles)}</p>
              </div>
              <Badge variant="secondary">{t($ => $.messages.unavailable)}</Badge>
            </div>
            <fieldset disabled aria-describedby={securityHintId} className="mt-3.5 min-w-0">
              <legend className="sr-only">{t($ => $.messages.reglagesDeSecuriteIndisponibles)}</legend>
              <UnavailableSwitch label={t($ => $.messages.authentificationMultifacteurObligatoire)} description={t($ => $.messages.exigerUnSecondFacteurPourAccederALOrganisation)} />
              <Field data-disabled className="mt-4.5">
                <FieldLabel htmlFor={ipsId}>{t($ => $.messages.adressesIpAutorisees)}</FieldLabel>
                <Textarea id={ipsId} disabled placeholder={t($ => $.messages.unavailable)} className="font-mono" />
              </Field>
            </fieldset>
          </Card>

          <Card className="gap-0 border-destructive/30 p-5">
            <div className="space-y-1">
              <h2 className="text-card-title text-destructive-strong">{t($ => $.messages.zoneSensible)}</h2>
              <p className="text-[10.5px] text-muted-foreground">{t($ => $.messages.laSuppressionDeLOrganisationEtDesAppartenancesDe)}</p>
            </div>
            <div className="mt-3.5 flex flex-col items-start gap-3 border-t border-destructive/20 pt-3.5 min-[621px]:flex-row min-[621px]:items-center min-[621px]:justify-between min-[621px]:gap-5">
              <div className="space-y-1 text-[10.5px] text-muted-foreground">
                <p>{t($ => $.messages.seuleUneOrganisationSansBucketNiCleDAcces)}</p>
                {counts && !empty && <p>{t($ => $.messages.cetteOrganisationContientEncoreDesRessourcesLaSuppressionDes)}</p>}
                {!owner && <p>{t($ => $.messages.seulsLesProprietairesPeuventSupprimerLOrganisation)}</p>}
              </div>
              {owner && <DeleteOrganizationDialog user={user} organization={current} empty={empty}
                disabled={!canMutate || mutation.isPending || !empty} restoreFocus={() => heading.current?.focus()} />}
            </div>
          </Card>
        </div>

        <aside aria-label={t($ => $.messages.recapitulatifDeLOrganisation)} className="grid min-w-0 gap-3.5 min-[761px]:grid-cols-2 min-[1161px]:sticky min-[1161px]:top-21 min-[1161px]:grid-cols-1">
          <Card className="gap-0 p-4" aria-labelledby={summaryId} aria-busy={details.isPending}>
            <h2 id={summaryId} className="break-words text-[14.5px] font-bold">{current.name}</h2>
            {details.isPending && <span role="status" className="sr-only">{t($ => $.messages.chargementDuRecapitulatif)}</span>}
            <dl className="mt-4 grid grid-cols-2 gap-x-3 gap-y-3.5">
              <div className="flex flex-col gap-1 text-muted-foreground">
                <dt className="text-[9px]">{t($ => $.messages.stockageUtilise)}</dt>
                <dd className="order-first text-[10.5px]">{t($ => $.messages.unavailable)}</dd>
              </div>
              {[
                { label: t($ => $.messages.members), count: counts?.members },
                { label: t($ => $.messages.buckets), count: counts?.buckets },
                { label: t($ => $.messages.credentials), count: counts?.credentials },
              ].map(({ label, count }) => <div key={label} className="flex min-w-0 flex-col gap-1">
                <dt className="text-[9px] text-muted-foreground">{label}</dt>
                <dd className="order-first font-display text-[17px] leading-tight font-bold tabular-nums">
                  {count !== undefined ? numberFormat.format(count) : details.isPending ? <Skeleton className="h-5 w-10" /> : <span className="font-sans text-[10.5px] font-normal text-muted-foreground">{t($ => $.messages.unavailable)}</span>}
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
  const { t } = useTranslation();
  const id = useId();
  const hintId = useId();
  return (
    <div className="flex items-start justify-between gap-6 py-3.5 last:pb-0">
      <div className="min-w-0 space-y-0.5 text-muted-foreground">
        <label htmlFor={id} className="block text-[11.5px] font-semibold">{label}</label>
        <p id={hintId} className="text-[10px]">{t($ => $.messages.descriptionIndisponible2, { description: description })}</p>
      </div>
      <Switch id={id} disabled checked={false} aria-describedby={hintId} className="mt-px" />
    </div>
  );
}
