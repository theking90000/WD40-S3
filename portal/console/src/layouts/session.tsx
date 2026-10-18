import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Outlet, useLoaderData, useRevalidator, useRouteError } from "react-router";
import { requireUserMiddleware, withUser } from "~/features/auth/router.ts";
import { AuthFailure, AuthPending } from "~/features/auth/components/auth-screen";
import { organizationErrorMessage } from "~/features/organizations/api.ts";
import { AuthError } from "~/lib/api.ts";
import { currentUserQuery } from "~/features/auth/queries.ts";

export const clientMiddleware = [requireUserMiddleware];
export const clientLoader = withUser();
export function shouldRevalidate() { return true; }
export { AuthPending as HydrateFallback } from "~/features/auth/components/auth-screen";

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
