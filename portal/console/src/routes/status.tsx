import { useState } from "react";
import { Button } from "~/components/ui/button";

export const handle = { title: "État du service" };

export default function Status() {
  const [status, setStatus] = useState("Non vérifié");

  async function check(): Promise<void> {
    setStatus("Vérification…");
    try {
      const response = await fetch(`${import.meta.env.BASE_URL}api/health`);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setStatus(await response.text());
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Service inaccessible");
    }
  }

  return (
    <section className="space-y-4">
      <h1 className="text-page-title">État du service</h1>
      <Button type="button" onClick={check}>Vérifier le service</Button>
      <p role="status">{status}</p>
    </section>
  );
}
