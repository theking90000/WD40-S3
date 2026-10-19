import { useId, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { PencilIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { OrganizationMember } from "~/features/organizations/members/api.ts";
import { memberErrorMessage, updateOrganizationMemberAccess } from "~/features/organizations/members/api.ts";
import { Button } from "~/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "~/components/ui/dialog";
import { Field, FieldDescription, FieldError, FieldLabel } from "~/components/ui/field";
import { NativeSelect, NativeSelectOption } from "~/components/ui/native-select";

export function EditMemberDialog({ user, organizationId, member, disabled, onUpdated, onError }: {
  user: CurrentUser;
  organizationId: string;
  member: OrganizationMember;
  disabled?: boolean;
  onUpdated(member: OrganizationMember): void;
  onError(error: Error): void;
}) {
  const [open, setOpen] = useState(false);
  const [access, setAccess] = useState(member.access);
  const selectId = useId();
  const hintId = useId();
  const errorId = useId();
  const mutation = useMutation({
    mutationFn: (value: OrganizationMember["access"]) => updateOrganizationMemberAccess(user, organizationId, member.email, value),
    retry: false,
    onSuccess: (updated) => {
      setOpen(false);
      onUpdated(updated);
    },
    onError,
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setAccess(member.access);
    setOpen(next);
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogTrigger asChild>
        <Button type="button" variant="ghost" size="icon-sm" disabled={disabled}
          aria-label={`Modifier le rôle de ${member.email}`} title="Modifier le rôle">
          <PencilIcon aria-hidden="true" className="size-3.5" />
        </Button>
      </DialogTrigger>
      <DialogContent showCloseButton={!mutation.isPending}>
        <DialogHeader>
          <DialogTitle>Modifier le rôle</DialogTitle>
          <DialogDescription className="break-words">Choisissez le rôle de {member.email} dans cette organisation.</DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={(event) => {
          event.preventDefault();
          if (!mutation.isPending && !disabled && access !== member.access) mutation.mutate(access);
        }}>
          <Field data-invalid={mutation.isError}>
            <FieldLabel htmlFor={selectId}>Rôle</FieldLabel>
            <NativeSelect id={selectId} name="access" value={access} disabled={mutation.isPending || disabled}
              aria-invalid={mutation.isError} aria-describedby={`${hintId}${mutation.isError ? ` ${errorId}` : ""}`}
              onChange={(event) => {
                const value = event.target.value;
                if (value === "MEMBER" || value === "OWNER") setAccess(value);
              }}>
              <NativeSelectOption value="MEMBER">Membre</NativeSelectOption>
              <NativeSelectOption value="OWNER">Propriétaire</NativeSelectOption>
            </NativeSelect>
            <FieldDescription id={hintId}>Les propriétaires peuvent ajouter ou retirer des membres et modifier leur rôle.</FieldDescription>
            {mutation.isError && <FieldError id={errorId}>{memberErrorMessage(mutation.error)}</FieldError>}
          </Field>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => changeOpen(false)} disabled={mutation.isPending}>Annuler</Button>
            <Button type="submit" disabled={mutation.isPending || disabled || access === member.access} aria-busy={mutation.isPending}>
              {mutation.isPending ? "Enregistrement en cours…" : "Enregistrer"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
