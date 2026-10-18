import { useId, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import type { CurrentUser } from "~/features/auth/session.ts";
import { createOrganization, organizationErrorMessage } from "~/features/organizations/api.ts";
import { Button } from "~/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "~/components/ui/dialog";
import { Field, FieldError, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";

export function CreateOrganizationDialog({ user, open, onOpenChange }: {
  user: CurrentUser;
  open: boolean;
  onOpenChange(open: boolean): void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Créer une organisation</DialogTitle>
          <DialogDescription>Une organisation regroupe votre espace de travail WD40-S3.</DialogDescription>
        </DialogHeader>
        <CreateOrganizationForm user={user} close={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  );
}

function CreateOrganizationForm({ user, close }: { user: CurrentUser; close(): void }) {
  const nameId = useId();
  const errorId = useId();
  const [name, setName] = useState("");
  const navigate = useNavigate();
  const mutation = useMutation({
    mutationFn: (value: string) => createOrganization(user, value),
    retry: false,
    onSuccess: async (organization) => {
      close();
      await navigate(`/${organization.id}/`);
    },
  });

  return (
    <form className="space-y-4" onSubmit={(event) => {
      event.preventDefault();
      if (!mutation.isPending) mutation.mutate(name);
    }}>
      <Field data-invalid={mutation.isError}>
        <FieldLabel htmlFor={nameId}>Nom de l'organisation</FieldLabel>
        <Input id={nameId} name="name" value={name} onChange={(event) => setName(event.target.value)}
          placeholder="Mon organisation" autoComplete="organization" required disabled={mutation.isPending}
          aria-invalid={mutation.isError} aria-describedby={mutation.isError ? errorId : undefined} />
        {mutation.isError && <FieldError id={errorId}>{organizationErrorMessage(mutation.error)}</FieldError>}
      </Field>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={close} disabled={mutation.isPending}>Annuler</Button>
        <Button type="submit" disabled={mutation.isPending || !name.trim()} aria-busy={mutation.isPending}>
          {mutation.isPending ? "Création en cours…" : "Créer l'organisation"}
        </Button>
      </DialogFooter>
    </form>
  );
}
