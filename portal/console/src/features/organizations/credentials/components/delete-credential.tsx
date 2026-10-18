import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Trash2Icon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Credential } from "~/features/organizations/credentials/api.ts";
import { credentialErrorMessage, deleteOrganizationCredential } from "~/features/organizations/credentials/api.ts";
import { Button } from "~/components/ui/button";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "~/components/ui/alert-dialog";

export function DeleteCredentialDialog({ user, organizationId, credential, disabled, onDeleted, restoreFocus }: {
  user: CurrentUser;
  organizationId: string;
  credential: Credential;
  disabled?: boolean;
  onDeleted(keyId: string): void;
  restoreFocus(): void;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const mutation = useMutation({
    mutationFn: () => deleteOrganizationCredential(user, organizationId, credential.key_id),
    retry: false,
    onSuccess: () => { setOpen(false); onDeleted(credential.key_id); },
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setOpen(next);
  }

  return (
    <AlertDialog open={open} onOpenChange={changeOpen}>
      <AlertDialogTrigger asChild>
        <Button ref={trigger} type="button" variant="ghost" size="icon-sm" disabled={disabled} title="Supprimer la clé"
          aria-label={`Supprimer la clé ${credential.key_id}`} className="hover:bg-destructive-soft hover:text-destructive-strong"><Trash2Icon aria-hidden="true" className="size-3.5" /></Button>
      </AlertDialogTrigger>
      <AlertDialogContent onCloseAutoFocus={(event) => {
        if (!trigger.current?.isConnected || trigger.current.disabled) {
          event.preventDefault();
          restoreFocus();
        }
      }}>
        <AlertDialogHeader>
          <AlertDialogTitle>Supprimer cette clé ?</AlertDialogTitle>
          <AlertDialogDescription className="break-words">La clé <span className="break-all font-mono">{credential.key_id}</span> et sa sélection de buckets seront supprimées du portail. Cette action est définitive.</AlertDialogDescription>
        </AlertDialogHeader>
        {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{credentialErrorMessage(mutation.error)}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={mutation.isPending}>Annuler</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={mutation.isPending || disabled} aria-busy={mutation.isPending} onClick={(event) => {
            event.preventDefault();
            if (!mutation.isPending && !disabled) mutation.mutate();
          }}>{mutation.isPending ? "Suppression…" : "Supprimer la clé"}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
