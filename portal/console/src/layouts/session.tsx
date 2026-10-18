import { Outlet, useRouteError } from "react-router";
import { requireUserMiddleware, withUser } from "~/auth-router.ts";
import { AuthFailure } from "~/components/auth/auth-screen";
import { organizationErrorMessage } from "~/organizations.ts";
import { AuthError } from "~/auth.ts";

export const clientMiddleware = [requireUserMiddleware];
export const clientLoader = withUser();
export function shouldRevalidate() { return true; }
export { AuthPending as HydrateFallback } from "~/components/auth/auth-screen";

export default function SessionLayout() {
  return <Outlet />;
}

export function ErrorBoundary() {
  const error = useRouteError();
  return (
    <AuthFailure
      title={error instanceof AuthError && error.code === "email_not_verified"
        ? "Vérifiez votre adresse email" : "Console indisponible"}
      description={organizationErrorMessage(error)}
    />
  );
}
