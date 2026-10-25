import { Trans, useTranslation } from "react-i18next";
import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { PencilIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Credential, CredentialInput } from "~/features/organizations/credentials/api.ts";
import { updateOrganizationCredentialAccess } from "~/features/organizations/credentials/api.ts";
import { CredentialAccessForm } from "./access-form";
import { Button } from "~/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "~/components/ui/dialog";

export function EditCredentialDialog({ user, organizationId, credential, disabled, onUpdated, restoreFocus }: {
  user: CurrentUser;
  organizationId: string;
  credential: Credential;
  disabled?: boolean;
  onUpdated(credential: Credential): void;
  restoreFocus(): void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const mutation = useMutation({
    mutationFn: (input: CredentialInput) => updateOrganizationCredentialAccess(user, organizationId, credential.key_id, input),
    retry: false,
    onSuccess: (updated) => { setOpen(false); onUpdated(updated); },
  });

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    mutation.reset();
    setOpen(next);
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogTrigger asChild>
        <Button ref={trigger} type="button" variant="ghost" size="icon-sm" disabled={disabled} title={t($ => $.messages.modifierLesAcces)} aria-label={t($ => $.messages.modifierLesAccesDeLaCleKeyId, { key_id: credential.key_id })}><PencilIcon aria-hidden="true" className="size-3.5" /></Button>
      </DialogTrigger>
      <DialogContent showCloseButton={!mutation.isPending} onCloseAutoFocus={(event) => {
        if (!trigger.current?.isConnected || trigger.current.disabled) {
          event.preventDefault();
          restoreFocus();
        }
      }}>
        <DialogHeader>
          <DialogTitle>{t($ => $.messages.modifierLesAcces)}</DialogTitle>
          <DialogDescription className="break-words"><Trans t={t} i18nKey={$ => $.messages.editCredentialDescription} values={{ keyId: credential.key_id }} components={{ key: <span className="break-all font-mono" /> }} /></DialogDescription>
        </DialogHeader>
        {open && <CredentialAccessForm user={user} organizationId={organizationId}
          initialInput={credential.access_type === "ALL"
            ? { access_type: "ALL", permissions: credential.permissions }
            : { access_type: "RESTRICTED", bucket_names: credential.bucket_names, permissions: credential.permissions }}
          pending={mutation.isPending} disabled={disabled} error={mutation.error} submitLabel={t($ => $.messages.enregistrer)}
          onEdit={() => { if (mutation.isError) mutation.reset(); }}
          onSubmit={(input) => mutation.mutate(input)} onCancel={() => changeOpen(false)} />}
      </DialogContent>
    </Dialog>
  );
}
