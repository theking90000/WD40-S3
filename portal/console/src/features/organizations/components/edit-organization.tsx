import { useId, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { PencilIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { organizationErrorMessage, updateOrganization } from "~/features/organizations/api.ts";
import { Button } from "~/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "~/components/ui/dialog";
import { Field, FieldError, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";

export function EditOrganizationDialog({ user, organization, disabled, onUpdated, restoreFocus }: {
  user: CurrentUser;
  organization: Organization;
  disabled?: boolean;
  onUpdated(): void;
  restoreFocus(): void;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(organization.name);
  const nameId = useId();
  const errorId = useId();
  const trigger = useRef<HTMLButtonElement | null>(null);
  const mutation = useMutation({
    mutationFn: (value: string) => updateOrganization(user, organization.id, value),
    retry: false,
    onSuccess: () => { setOpen(false); onUpdated(); },
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setName(organization.name);
    setOpen(next);
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogTrigger asChild>
        <Button ref={trigger} type="button" variant="outline" disabled={disabled}><PencilIcon aria-hidden="true" />Renommer</Button>
      </DialogTrigger>
      <DialogContent showCloseButton={!mutation.isPending} onCloseAutoFocus={(event) => {
        if (!trigger.current?.isConnected || trigger.current.disabled) {
          event.preventDefault();
          restoreFocus();
        }
      }}>
        <DialogHeader>
          <DialogTitle>Renommer l'organisation</DialogTitle>
          <DialogDescription>Le nouveau nom sera visible par tous les membres de l'organisation.</DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={(event) => {
          event.preventDefault();
          if (!mutation.isPending && !disabled && name.trim() !== organization.name) mutation.mutate(name);
        }}>
          <Field data-invalid={mutation.isError}>
            <FieldLabel htmlFor={nameId}>Nom de l'organisation</FieldLabel>
            <Input id={nameId} name="name" value={name} autoComplete="organization" required
              disabled={mutation.isPending || disabled} aria-invalid={mutation.isError}
              aria-describedby={mutation.isError ? errorId : undefined} onChange={(event) => {
                setName(event.target.value);
                if (mutation.isError) mutation.reset();
              }} />
            {mutation.isError && <FieldError id={errorId}>{organizationErrorMessage(mutation.error)}</FieldError>}
          </Field>
          <DialogFooter>
            <Button type="button" variant="outline" disabled={mutation.isPending} onClick={() => changeOpen(false)}>Annuler</Button>
            <Button type="submit" disabled={mutation.isPending || disabled || !name.trim() || name.trim() === organization.name}
              aria-busy={mutation.isPending}>{mutation.isPending ? "Enregistrement en cours…" : "Enregistrer"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
