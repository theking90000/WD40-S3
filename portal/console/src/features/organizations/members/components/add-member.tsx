import { useId, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { PlusIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import { addOrganizationMember, memberErrorMessage } from "~/features/organizations/members/api.ts";
import { Button } from "~/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "~/components/ui/dialog";
import { Field, FieldDescription, FieldError, FieldLabel } from "~/components/ui/field";
import { Input } from "~/components/ui/input";

export function AddMemberDialog({ user, organizationId, disabled, variant = "default", onAdded, onError }: {
  user: CurrentUser;
  organizationId: string;
  disabled?: boolean;
  variant?: "default" | "outline";
  onAdded(email: string): void;
  onError(error: Error): void;
}) {
  const [open, setOpen] = useState(false);
  const [email, setEmail] = useState("");
  const inputId = useId();
  const hintId = useId();
  const errorId = useId();
  const mutation = useMutation({
    mutationFn: (value: string) => addOrganizationMember(user, organizationId, value),
    retry: false,
    onSuccess: (member) => {
      setOpen(false);
      setEmail("");
      onAdded(member.email);
    },
    onError,
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setEmail("");
    setOpen(next);
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogTrigger asChild>
        <Button type="button" variant={variant} disabled={disabled}><PlusIcon aria-hidden="true" />Ajouter un membre</Button>
      </DialogTrigger>
      <DialogContent showCloseButton={!mutation.isPending}>
        <DialogHeader>
          <DialogTitle>Ajouter un membre</DialogTitle>
          <DialogDescription>Donnez accès à cette organisation avec une adresse email.</DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={(event) => {
          event.preventDefault();
          if (!mutation.isPending) mutation.mutate(email);
        }}>
          <Field data-invalid={mutation.isError}>
            <FieldLabel htmlFor={inputId}>Adresse email</FieldLabel>
            <Input id={inputId} name="email" type="email" autoComplete="email" placeholder="prenom@entreprise.fr"
              value={email} onChange={(event) => setEmail(event.target.value)} required disabled={mutation.isPending}
              aria-invalid={mutation.isError} aria-describedby={`${hintId}${mutation.isError ? ` ${errorId}` : ""}`} />
            <FieldDescription id={hintId}>
              L'accès à la gestion de l'organisation est accordé au compte possédant cette adresse email.
            </FieldDescription>
            {mutation.isError && <FieldError id={errorId}>{memberErrorMessage(mutation.error)}</FieldError>}
          </Field>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => changeOpen(false)} disabled={mutation.isPending}>Annuler</Button>
            <Button type="submit" disabled={mutation.isPending || !email.trim()} aria-busy={mutation.isPending}>
              {mutation.isPending ? "Ajout en cours…" : "Ajouter le membre"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
