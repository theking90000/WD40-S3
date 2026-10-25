import type { Message } from "~/features/i18n/message";
import { useTranslation } from "react-i18next";
import { useEffect, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { CheckIcon, CopyIcon } from "lucide-react";
import type { CurrentUser } from "~/features/auth/session.ts";
import type { Organization } from "~/features/organizations/api.ts";
import type { Bucket, BucketAccess } from "~/features/organizations/buckets/api.ts";
import { bucketErrorKey, updateOrganizationBucketAccess } from "~/features/organizations/buckets/api.ts";
import { BucketAccessBadge, BucketAccessField, useBucketAccessInfo } from "./access";
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
  const { t } = useTranslation();
  const bucketAccessInfo = useBucketAccessInfo();
  const [draftAccess, setDraftAccess] = useState<BucketAccess | null>(null);
  const [notice, setNotice] = useState<Message | null>(null);
  const [copyError, setCopyError] = useState<Message | null>(null);
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
      setNotice(null);
      setCopyError(null);
      setCopied(false);
      mutation.reset();
    }
    onOpenChange(next);
  }

  async function copyName() {
    setNotice(null);
    setCopyError(null);
    try {
      await navigator.clipboard.writeText(bucket.name);
      setCopied(true);
    } catch {
      setCopyError({ key: "copieImpossibleSelectionnezLeNomCiDessusPourLe" });
    }
  }

  return (
    <Sheet open={open} onOpenChange={changeOpen}>
      <SheetContent showCloseButton={!mutation.isPending} onCloseAutoFocus={(event) => {
        event.preventDefault();
        restoreFocus();
      }}>
        <SheetHeader>
          <p className="text-[10.5px] font-semibold text-muted-foreground">{t($ => $.messages.bucket)}</p>
          <SheetTitle className="break-all">{bucket.name}</SheetTitle>
          <SheetDescription>{t($ => $.messages.configurationDuBucketDansName, { name: organization.name })}</SheetDescription>
        </SheetHeader>
        <div className="space-y-6 p-5">
          <dl className="grid grid-cols-2 gap-3 text-xs">
            <div className="space-y-2 rounded-lg border bg-background p-3">
              <dt className="text-[10px] text-muted-foreground">{t($ => $.messages.createdAt)}</dt>
              <dd><time dateTime={bucket.created_at}>{new Intl.DateTimeFormat("fr-BE", { dateStyle: "medium" }).format(new Date(bucket.created_at))}</time></dd>
            </div>
            <div className="space-y-2 rounded-lg border bg-background p-3">
              <dt className="text-[10px] text-muted-foreground">{t($ => $.messages.accesEnregistre)}</dt>
              <dd><BucketAccessBadge access={bucket.access} /></dd>
            </div>
          </dl>
          <div className="space-y-2">
            <h3 className="text-xs font-semibold">{t($ => $.messages.nomDuBucket)}</h3>
            <code className="block select-all break-all rounded-lg border bg-background p-3 font-mono text-xs">{bucket.name}</code>
            <p className="text-[10.5px] text-muted-foreground">{t($ => $.messages.leNomNePeutPasEtreModifie)}</p>
          </div>
          <S3Connection bucketName={bucket.name} compact />
          {owner ? (
            <form className="space-y-4" onSubmit={(event) => {
              event.preventDefault();
              if (!mutation.isPending && !disabled && access !== bucket.access) {
                mutation.mutate(access, { onSuccess: (updated) => {
                  setDraftAccess(null);
                  setNotice({ key: "leNiveauDAccesAEteEnregistre" });
                  setCopyError(null);
                  onUpdated(updated);
                } });
              }
            }}>
              <BucketAccessField value={access} disabled={mutation.isPending || disabled} onChange={(value) => {
                setDraftAccess(value);
                setNotice(null);
                if (mutation.isError) mutation.reset();
              }} />
              {mutation.isError && <p role="alert" className="text-xs text-destructive-strong">{t($ => $.messages[bucketErrorKey(mutation.error)])}</p>}
              <Button type="submit" disabled={mutation.isPending || disabled || access === bucket.access} aria-busy={mutation.isPending}>
                {mutation.isPending ? t($ => $.messages.enregistrement) : t($ => $.messages.enregistrerLAcces)}
              </Button>
            </form>
          ) : <p className="text-xs text-muted-foreground">{t($ => $.messages.descriptionSeulsLesProprietairesPeuventModifierLAcces2, { description: bucketAccessInfo[bucket.access].description })}</p>}
          {notice && <p role="status" className="rounded-lg border bg-success-soft p-3 text-xs text-success-strong">{t($ => $.messages[notice.key], notice.values)}</p>}
          <span role="status" className="sr-only">{copied ? t($ => $.messages.nomDuBucketCopie) : ""}</span>
          {copyError && <p role="alert" className="text-xs text-destructive-strong">{t($ => $.messages[copyError.key], copyError.values)}</p>}
        </div>
        <SheetFooter>
          <Button type="button" variant="outline" onClick={() => void copyName()}>
            {copied ? <CheckIcon aria-hidden="true" /> : <CopyIcon aria-hidden="true" />}{t($ => $.messages.copierLeNomDuBucket)}</Button>
          <Button type="button" variant="ghost" disabled={mutation.isPending} onClick={() => changeOpen(false)}>{t($ => $.messages.close)}</Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
