import { useTranslation } from "react-i18next";
import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { Trash2Icon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import { deleteOrganization, organizationErrorKey } from "~/features/organizations/api.ts";
import { Button } from "~/components/ui/button";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "~/components/ui/alert-dialog";

export function DeleteOrganizationDialog({ user, organization, disabled, empty, restoreFocus }: {
  user: CurrentUser;
  organization: Organization;
  disabled?: boolean;
  empty: boolean;
  restoreFocus(): void;
}) {
  const { t } = useTranslation();
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
          className="w-full min-[621px]:w-auto"><Trash2Icon aria-hidden="true" />{t($ => $.messages.supprimerLOrganisation)}</Button>
      </AlertDialogTrigger>
      <AlertDialogContent onCloseAutoFocus={(event) => {
        if (!trigger.current?.isConnected || trigger.current.disabled) {
          event.preventDefault();
          restoreFocus();
        }
      }}>
        <AlertDialogHeader>
          <AlertDialogTitle>{t($ => $.messages.supprimerCetteOrganisation)}</AlertDialogTitle>
          <AlertDialogDescription className="break-words">{t($ => $.messages.lOrganisationNameEtLesAppartenancesDeTousSes2, { name: organization.name })}</AlertDialogDescription>
        </AlertDialogHeader>
        {!empty && <p className="text-xs text-muted-foreground">{t($ => $.messages.laSuppressionExigeUneOrganisationSansBucketNiCle)}</p>}
        {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{t($ => $.messages[organizationErrorKey(mutation.error)])}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={mutation.isPending}>{t($ => $.messages.cancel)}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={mutation.isPending || disabled || !empty} aria-busy={mutation.isPending}
            onClick={(event) => {
              event.preventDefault();
              if (!mutation.isPending && !disabled && empty) mutation.mutate();
            }}>{mutation.isPending ? t($ => $.messages.suppressionEnCours) : t($ => $.messages.supprimerLOrganisation)}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
