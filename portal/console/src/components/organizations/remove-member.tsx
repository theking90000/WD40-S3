import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Trash2Icon } from "lucide-react";
import type { CurrentUser } from "~/auth.ts";
import type { OrganizationMember } from "~/organization-members.ts";
import { memberErrorMessage, removeOrganizationMember } from "~/organization-members.ts";
import { Button } from "~/components/ui/button";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger,
} from "~/components/ui/alert-dialog";

export function RemoveMemberDialog({ user, organizationId, member, onRemoved, onError }: {
  user: CurrentUser;
  organizationId: string;
  member: OrganizationMember;
  onRemoved(email: string): void;
  onError(error: Error): void;
}) {
  const [open, setOpen] = useState(false);
  const mutation = useMutation({
    mutationFn: () => removeOrganizationMember(user, organizationId, member.email),
    retry: false,
    onSuccess: () => {
      setOpen(false);
      onRemoved(member.email);
    },
    onError,
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setOpen(next);
  }

  return (
    <AlertDialog open={open} onOpenChange={changeOpen}>
      <AlertDialogTrigger asChild>
        <Button type="button" variant="ghost" size="icon-sm" aria-label={`Retirer ${member.email}`}
          className="hover:bg-destructive-soft hover:text-destructive-strong">
          <Trash2Icon aria-hidden="true" className="size-3.5" />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Retirer ce membre ?</AlertDialogTitle>
          <AlertDialogDescription className="break-words">
            {member.email} perdra immédiatement l'accès à cette organisation et à ses ressources.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{memberErrorMessage(mutation.error)}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={mutation.isPending}>Annuler</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={mutation.isPending} aria-busy={mutation.isPending}
            onClick={(event) => {
              event.preventDefault();
              if (!mutation.isPending) mutation.mutate();
            }}>
            {mutation.isPending ? "Retrait en cours…" : "Retirer le membre"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
