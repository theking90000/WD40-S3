import type { ReactNode } from "react";
import { isRouteErrorResponse, Links, Meta, Outlet, Scripts, ScrollRestoration, useRouteError } from "react-router";
import { authMiddleware } from "./auth-router.ts";
import { AuthFailure } from "~/components/auth/auth-screen";
import "./style.css";

export const clientMiddleware = [authMiddleware];

// Keep public prerendered HTML while running the callback middleware on hydration.
export function loader() {
  return null;
}

export function clientLoader() {
  return null;
}
clientLoader.hydrate = true as const;

export function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="fr">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>WD40 Portal</title>
        <Meta />
        <Links />
      </head>
      <body>
        {children}
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  );
}

export default function App() {
  return <Outlet />;
}

export function ErrorBoundary() {
  const error = useRouteError();
  if (isRouteErrorResponse(error) && error.status === 404) {
    return <AuthFailure title="Page introuvable" description="Cette page n'existe pas." />;
  }
  return <AuthFailure />;
}
