import { useState } from "react";
import { UserRoundIcon } from "lucide-react";
import { useNavigate } from "react-router";
import { logout } from "~/features/auth/session.ts";
import type { CurrentUser } from "~/features/auth/session.ts";
import { Button } from "~/components/ui/button";

export function Account({ user }: { user: CurrentUser }) {
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  async function disconnect() {
    setBusy(true);
    setError(false);
    try {
      await logout();
      await navigate("/login", { replace: true });
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex min-w-0 items-center gap-2.25">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-full border bg-foreground-soft text-muted-foreground">
          <UserRoundIcon aria-hidden="true" className="size-3.5" strokeWidth={1.6} />
        </span>
        <div className="min-w-0">
          <p className="truncate text-[11px] font-semibold">{user.name ?? user.email ?? user.subject}</p>
          {user.name && user.email && <p className="truncate text-[9.5px] text-muted-foreground">{user.email}</p>}
        </div>
      </div>
      <Button type="button" variant="outline" size="sm" className="w-full" onClick={disconnect} disabled={busy} aria-busy={busy}>
        {busy ? "En cours…" : "Se déconnecter"}
      </Button>
      {error && <p role="alert" className="text-[10.5px] text-destructive-strong">Impossible de vous déconnecter. Réessayez dans un instant.</p>}
    </div>
  );
}
