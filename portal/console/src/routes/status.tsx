import { useTranslation } from "react-i18next";
import { routeTitle } from "~/features/i18n/route-titles";
import { useState } from "react";
import { Button } from "~/components/ui/button";

export const handle = routeTitle("status");

type ServiceStatus = "unchecked" | "checking" | { response: string } | { error: unknown };

export default function Status() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ServiceStatus>("unchecked");

  async function check(): Promise<void> {
    setStatus("checking");
    try {
      const response = await fetch(`${import.meta.env.BASE_URL}api/health`);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setStatus({ response: await response.text() });
    } catch (error) {
      setStatus({ error });
    }
  }

  return (
    <section className="space-y-4">
      <h1 className="text-page-title">{t($ => $.messages.serviceStatus)}</h1>
      <Button type="button" onClick={check}>{t($ => $.messages.verifierLeService)}</Button>
      <p role="status">{status === "unchecked" ? t($ => $.messages.nonVerifie)
        : status === "checking" ? t($ => $.messages.verification)
          : "response" in status ? status.response
            : status.error instanceof Error ? status.error.message : t($ => $.messages.serviceInaccessible)}</p>
    </section>
  );
}
