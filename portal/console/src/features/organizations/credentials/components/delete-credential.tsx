import { Trans, useTranslation } from "react-i18next";
import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Trash2Icon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Credential } from "~/features/organizations/credentials/api.ts";
import { credentialErrorKey, deleteOrganizationCredential } from "~/features/organizations/credentials/api.ts";
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
  const { t } = useTranslation();
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
        <Button ref={trigger} type="button" variant="ghost" size="icon-sm" disabled={disabled} title={t($ => $.messages.supprimerLaCle)}
          aria-label={t($ => $.messages.supprimerLaCleKeyId, { key_id: credential.key_id })} className="hover:bg-destructive-soft hover:text-destructive-strong"><Trash2Icon aria-hidden="true" className="size-3.5" /></Button>
      </AlertDialogTrigger>
      <AlertDialogContent onCloseAutoFocus={(event) => {
        if (!trigger.current?.isConnected || trigger.current.disabled) {
          event.preventDefault();
          restoreFocus();
        }
      }}>
        <AlertDialogHeader>
          <AlertDialogTitle>{t($ => $.messages.supprimerCetteCle)}</AlertDialogTitle>
          <AlertDialogDescription className="break-words"><Trans t={t} i18nKey={$ => $.messages.deleteCredentialDescription} values={{ keyId: credential.key_id }} components={{ key: <span className="break-all font-mono" /> }} /></AlertDialogDescription>
        </AlertDialogHeader>
        {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{t($ => $.messages[credentialErrorKey(mutation.error)])}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={mutation.isPending}>{t($ => $.messages.cancel)}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={mutation.isPending || disabled} aria-busy={mutation.isPending} onClick={(event) => {
            event.preventDefault();
            if (!mutation.isPending && !disabled) mutation.mutate();
          }}>{mutation.isPending ? t($ => $.messages.suppression2) : t($ => $.messages.supprimerLaCle)}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
