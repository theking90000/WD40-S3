import { useId } from "react";
import type { BucketAccess } from "~/features/organizations/buckets/api.ts";
import { Badge } from "~/components/ui/badge";
import { Field, FieldDescription, FieldLabel } from "~/components/ui/field";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";

export const bucketAccessInfo: Record<BucketAccess, { label: string; description: string }> = {
  PRIVATE: {
    label: "Privé",
    description: "Une authentification est requise pour toutes les opérations.",
  },
  PUBLIC: {
    label: "Public",
    description: "GET et HEAD sur les objets sans clé. Le listing et les autres opérations exigent une authentification.",
  },
  PUBLIC_LIST: {
    label: "Public avec listing",
    description: "GET et HEAD sur les objets, ainsi que LIST, sans clé. Les autres opérations exigent une authentification.",
  },
};

export function BucketAccessBadge({ access }: { access: BucketAccess }) {
  return <Badge variant={access === "PRIVATE" ? "neutral" : "warning"}>{bucketAccessInfo[access].label}</Badge>;
}

export function BucketAccessField({ value, onChange, disabled }: {
  value: BucketAccess;
  onChange(value: BucketAccess): void;
  disabled?: boolean;
}) {
  const id = useId();
  const hintId = useId();
  return (
    <Field>
      <FieldLabel htmlFor={id}>Accès</FieldLabel>
      <NativeSelect id={id} name="access" value={value} disabled={disabled} aria-describedby={hintId}
        onChange={(event) => {
          const next = event.target.value;
          if (next === "PRIVATE" || next === "PUBLIC" || next === "PUBLIC_LIST") onChange(next);
        }}>
        <NativeSelectOption value="PRIVATE">Privé</NativeSelectOption>
        <NativeSelectOption value="PUBLIC">Public</NativeSelectOption>
        <NativeSelectOption value="PUBLIC_LIST">Public avec listing</NativeSelectOption>
      </NativeSelect>
      <FieldDescription id={hintId}>{bucketAccessInfo[value].description}</FieldDescription>
    </Field>
  );
}
