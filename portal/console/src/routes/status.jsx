import { useState } from "react";

export default function Status() {
  const [status, setStatus] = useState("Non vérifié");

  async function check() {
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
    <section>
      <h2>État du service</h2>
      <button type="button" onClick={check}>Vérifier le service</button>
      <p role="status">{status}</p>
    </section>
  );
}
