import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Outlet, useLoaderData, useRevalidator, useRouteError } from "react-router";
import { requireUserMiddleware, withUser } from "~/auth-router.ts";
import { AuthFailure, AuthPending } from "~/components/auth/auth-screen";
import { organizationErrorMessage } from "~/organizations.ts";
import { AuthError } from "~/auth.ts";
import { currentUserQuery } from "~/current-user.ts";

export const clientMiddleware = [requireUserMiddleware];
export const clientLoader = withUser();
export function shouldRevalidate() { return true; }
export { AuthPending as HydrateFallback } from "~/components/auth/auth-screen";

export default function SessionLayout() {
  const { user: loadedUser } = useLoaderData<typeof clientLoader>();
  const { data: user } = useQuery({ ...currentUserQuery(), enabled: false });
  const revalidator = useRevalidator();
  const changed = user !== undefined && user !== loadedUser;

  useEffect(() => {
    if (changed && revalidator.state === "idle") void revalidator.revalidate();
  }, [changed, revalidator]);

  return changed ? <AuthPending /> : <Outlet />;
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
