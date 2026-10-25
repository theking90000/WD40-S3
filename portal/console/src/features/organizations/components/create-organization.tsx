import { useTranslation } from "react-i18next";
import { useId, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import type { CurrentUser } from "~/features/auth/session.ts";
import { createOrganization, organizationErrorKey } from "~/features/organizations/api.ts";
import { Button } from "~/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "~/components/ui/dialog";
import { Field, FieldError, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";

export function CreateOrganizationDialog({ user, open, onOpenChange }: {
  user: CurrentUser;
  open: boolean;
  onOpenChange(open: boolean): void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t($ => $.messages.creerUneOrganisation)}</DialogTitle>
          <DialogDescription>{t($ => $.messages.uneOrganisationRegroupeVotreEspaceDeTravailWd40S3)}</DialogDescription>
        </DialogHeader>
        <CreateOrganizationForm user={user} close={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  );
}

function CreateOrganizationForm({ user, close }: { user: CurrentUser; close(): void }) {
  const { t } = useTranslation();
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
        <FieldLabel htmlFor={nameId}>{t($ => $.messages.nomDeLOrganisation)}</FieldLabel>
        <Input id={nameId} name="name" value={name} onChange={(event) => {
          setName(event.target.value);
          if (mutation.isError) mutation.reset();
        }}
          placeholder={t($ => $.messages.monOrganisation)} autoComplete="organization" required disabled={mutation.isPending}
          aria-invalid={mutation.isError} aria-describedby={mutation.isError ? errorId : undefined} />
        {mutation.isError && <FieldError id={errorId}>{t($ => $.messages[organizationErrorKey(mutation.error)])}</FieldError>}
      </Field>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={close} disabled={mutation.isPending}>{t($ => $.messages.cancel)}</Button>
        <Button type="submit" disabled={mutation.isPending || !name.trim()} aria-busy={mutation.isPending}>
          {mutation.isPending ? t($ => $.messages.creationEnCours) : t($ => $.messages.creerLOrganisation)}
        </Button>
      </DialogFooter>
    </form>
  );
}
