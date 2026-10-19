import { useEffect, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { CheckIcon, CopyIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { Bucket, BucketAccess } from "~/features/organizations/buckets/api.ts";
import { bucketErrorMessage, updateOrganizationBucketAccess } from "~/features/organizations/buckets/api.ts";
import { BucketAccessBadge, BucketAccessField, bucketAccessInfo } from "./access";
import { Button } from "~/components/ui/button";
import { S3Connection } from "~/components/s3-connection";
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "~/components/ui/sheet";

export function BucketDetails({ user, organization, bucket, open, disabled, onOpenChange, onUpdated, restoreFocus }: {
  user: CurrentUser;
  organization: Organization;
  bucket: Bucket;
  open: boolean;
  disabled?: boolean;
  onOpenChange(open: boolean): void;
  onUpdated(bucket: Bucket): void;
  restoreFocus(): void;
}) {
  const [draftAccess, setDraftAccess] = useState<BucketAccess | null>(null);
  const [notice, setNotice] = useState("");
  const [copyError, setCopyError] = useState("");
  const [copied, setCopied] = useState(false);
  const access = draftAccess ?? bucket.access;
  const owner = organization.access === "OWNER";
  const mutation = useMutation({
    mutationFn: (value: BucketAccess) => updateOrganizationBucketAccess(user, organization.id, bucket.name, value),
    retry: false,
  });

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  function changeOpen(next: boolean) {
    if (mutation.isPending) return;
    if (!next) {
      setDraftAccess(null);
      setNotice("");
      setCopyError("");
      setCopied(false);
      mutation.reset();
    }
    onOpenChange(next);
  }

  async function copyName() {
    setNotice("");
    setCopyError("");
    try {
      await navigator.clipboard.writeText(bucket.name);
      setCopied(true);
    } catch {
      setCopyError("Copie impossible. Sélectionnez le nom ci-dessus pour le copier manuellement.");
    }
  }

  return (
    <Sheet open={open} onOpenChange={changeOpen}>
      <SheetContent showCloseButton={!mutation.isPending} onCloseAutoFocus={(event) => {
        event.preventDefault();
        restoreFocus();
      }}>
        <SheetHeader>
          <p className="text-[10.5px] font-semibold text-muted-foreground">Bucket</p>
          <SheetTitle className="break-all">{bucket.name}</SheetTitle>
          <SheetDescription>Configuration du bucket dans {organization.name}.</SheetDescription>
        </SheetHeader>
        <div className="space-y-6 p-5">
          <dl className="grid grid-cols-2 gap-3 text-xs">
            <div className="space-y-2 rounded-lg border bg-background p-3">
              <dt className="text-[10px] text-muted-foreground">Créé le</dt>
              <dd><time dateTime={bucket.created_at}>{new Intl.DateTimeFormat("fr-BE", { dateStyle: "medium" }).format(new Date(bucket.created_at))}</time></dd>
            </div>
            <div className="space-y-2 rounded-lg border bg-background p-3">
              <dt className="text-[10px] text-muted-foreground">Accès enregistré</dt>
              <dd><BucketAccessBadge access={bucket.access} /></dd>
            </div>
          </dl>
          <div className="space-y-2">
            <h3 className="text-xs font-semibold">Nom du bucket</h3>
            <code className="block select-all break-all rounded-lg border bg-background p-3 font-mono text-xs">{bucket.name}</code>
            <p className="text-[10.5px] text-muted-foreground">Le nom ne peut pas être modifié.</p>
          </div>
          <S3Connection bucketName={bucket.name} compact />
          {owner ? (
            <form className="space-y-4" onSubmit={(event) => {
              event.preventDefault();
              if (!mutation.isPending && !disabled && access !== bucket.access) {
                mutation.mutate(access, { onSuccess: (updated) => {
                  setDraftAccess(null);
                  setNotice("Le niveau d'accès a été enregistré.");
                  setCopyError("");
                  onUpdated(updated);
                } });
              }
            }}>
              <BucketAccessField value={access} disabled={mutation.isPending || disabled} onChange={(value) => {
                setDraftAccess(value);
                setNotice("");
                if (mutation.isError) mutation.reset();
              }} />
              {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{bucketErrorMessage(mutation.error)}</p>}
              <Button type="submit" disabled={mutation.isPending || disabled || access === bucket.access} aria-busy={mutation.isPending}>
                {mutation.isPending ? "Enregistrement…" : "Enregistrer l'accès"}
              </Button>
            </form>
          ) : <p className="text-xs text-muted-foreground">{bucketAccessInfo[bucket.access].description} Seuls les propriétaires peuvent modifier l'accès.</p>}
          {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{notice}</p>}
          <span role="status" className="sr-only">{copied ? "Nom du bucket copié." : ""}</span>
          {copyError && <p role="alert" className="text-xs text-destructive-strong">{copyError}</p>}
        </div>
        <SheetFooter>
          <Button type="button" variant="outline" onClick={() => void copyName()}>
            {copied ? <CheckIcon aria-hidden="true" /> : <CopyIcon aria-hidden="true" />}Copier le nom du bucket
          </Button>
          <Button type="button" variant="ghost" disabled={mutation.isPending} onClick={() => changeOpen(false)}>Fermer</Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
