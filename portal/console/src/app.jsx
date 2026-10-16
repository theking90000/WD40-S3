import { useState } from "react";
import { createRoot } from "react-dom/client";
import "./style.css";

function App() {
  const [status, setStatus] = useState("Non vérifié");

  async function check() {
    setStatus("Vérification…");
    try {
      const response = await fetch(new URL("api/health", document.baseURI));
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setStatus(await response.text());
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Service inaccessible");
    }
  }

  return (
    <main>
      <h1>WD40 Portal</h1>
      <p>Console de gestion. Authentification et opérations WD40 à venir.</p>
      <button type="button" onClick={check}>Vérifier le service</button>
      <p role="status">{status}</p>
    </main>
  );
}

createRoot(document.getElementById("root")).render(<App />);
