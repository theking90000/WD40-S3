import { useTranslation } from "react-i18next";
import { useId } from "react";
import type { BucketAccess } from "~/features/organizations/buckets/api.ts";
import { Badge } from "~/components/ui/badge";
import { Field, FieldDescription, FieldLabel } from "~/components/ui/field";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";

export function useBucketAccessInfo(): Record<BucketAccess, { label: string; description: string }> {
  const { t } = useTranslation();
  return {
    PRIVATE: {
      label: t($ => $.messages.prive),
      description: t($ => $.messages.uneAuthentificationEstRequisePourToutesLesOperations),
    },
    PUBLIC: {
      label: t($ => $.messages.public),
      description: t($ => $.messages.getEtHeadSurLesObjetsSansCleLe),
    },
    PUBLIC_LIST: {
      label: t($ => $.messages.publicAvecListing),
      description: t($ => $.messages.getEtHeadSurLesObjetsAinsiQueList),
    },
  };
}

export function BucketAccessBadge({ access }: { access: BucketAccess }) {
  const info = useBucketAccessInfo();
  return <Badge variant={access === "PRIVATE" ? "neutral" : "warning"}>{info[access].label}</Badge>;
}

export function BucketAccessField({ value, onChange, disabled }: {
  value: BucketAccess;
  onChange(value: BucketAccess): void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const bucketAccessInfo = useBucketAccessInfo();
  const id = useId();
  const hintId = useId();
  return (
    <Field>
      <FieldLabel htmlFor={id}>{t($ => $.messages.access)}</FieldLabel>
      <NativeSelect id={id} name="access" value={value} disabled={disabled} aria-describedby={hintId}
        onChange={(event) => {
          const next = event.target.value;
          if (next === "PRIVATE" || next === "PUBLIC" || next === "PUBLIC_LIST") onChange(next);
        }}>
        <NativeSelectOption value="PRIVATE">{t($ => $.messages.prive)}</NativeSelectOption>
        <NativeSelectOption value="PUBLIC">{t($ => $.messages.public)}</NativeSelectOption>
        <NativeSelectOption value="PUBLIC_LIST">{t($ => $.messages.publicAvecListing)}</NativeSelectOption>
      </NativeSelect>
      <FieldDescription id={hintId}>{bucketAccessInfo[value].description}</FieldDescription>
    </Field>
  );
}
