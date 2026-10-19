import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { Trash2Icon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { deleteOrganization, organizationErrorMessage } from "~/features/organizations/api.ts";
import { Button } from "~/components/ui/button";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "~/components/ui/alert-dialog";

export function DeleteOrganizationDialog({ user, organization, disabled, empty, restoreFocus }: {
  user: CurrentUser;
  organization: Organization;
  disabled?: boolean;
  empty: boolean;
  restoreFocus(): void;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const navigate = useNavigate();
  const mutation = useMutation({
    mutationFn: () => deleteOrganization(user, organization.id),
    retry: false,
    onSuccess: () => {
      setOpen(false);
      void navigate("/?organization_deleted=1", { replace: true });
    },
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setOpen(next);
  }

  return (
    <AlertDialog open={open} onOpenChange={changeOpen}>
      <AlertDialogTrigger asChild>
        <Button ref={trigger} type="button" variant="destructive" disabled={disabled}
          className="w-full min-[621px]:w-auto"><Trash2Icon aria-hidden="true" />Supprimer l'organisation</Button>
      </AlertDialogTrigger>
      <AlertDialogContent onCloseAutoFocus={(event) => {
        if (!trigger.current?.isConnected || trigger.current.disabled) {
          event.preventDefault();
          restoreFocus();
        }
      }}>
        <AlertDialogHeader>
          <AlertDialogTitle>Supprimer cette organisation ?</AlertDialogTitle>
          <AlertDialogDescription className="break-words">L'organisation {organization.name} et les appartenances de tous ses membres seront supprimées. Cette action est définitive.</AlertDialogDescription>
        </AlertDialogHeader>
        {!empty && <p className="text-xs text-muted-foreground">La suppression exige une organisation sans bucket ni clé d'accès. La suppression des buckets n'est pas encore disponible dans le portail.</p>}
        {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{organizationErrorMessage(mutation.error)}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={mutation.isPending}>Annuler</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={mutation.isPending || disabled || !empty} aria-busy={mutation.isPending}
            onClick={(event) => {
              event.preventDefault();
              if (!mutation.isPending && !disabled && empty) mutation.mutate();
            }}>{mutation.isPending ? "Suppression en cours…" : "Supprimer l'organisation"}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
