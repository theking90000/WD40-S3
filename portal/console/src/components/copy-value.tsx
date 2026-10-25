import { useTranslation } from "react-i18next";
import { useEffect, useState } from "react";
import { CheckIcon, CopyIcon } from "lucide-react";
import { Button } from "~/components/ui/button";

export function CopyValue({ value, label, compact = false }: { value: string; label: string; compact?: boolean }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  async function copy() {
    setError(false);
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
    } catch {
      setError(true);
    }
  }

  return (
    <span className="inline-flex max-w-full flex-col items-start gap-1">
      <Button type="button" variant={compact ? "ghost" : "outline"} size={compact ? "icon-sm" : "sm"}
        aria-label={copied ? t($ => $.messages.copied) : label} title={copied ? t($ => $.messages.copied) : label} onClick={() => void copy()}>
        {copied ? <CheckIcon aria-hidden="true" className="size-3.5" /> : <CopyIcon aria-hidden="true" className="size-3.5" />}
        {!compact && label}
      </Button>
      <span role="status" className="sr-only">{copied ? t($ => $.messages.copySucceeded) : ""}</span>
      {error && <span role="alert" className="break-words text-[10px] text-destructive-strong">{t($ => $.messages.copyFailed)}</span>}
    </span>
  );
}
