import { createContext, replace } from "react-router";
import type { LoaderFunctionArgs, MiddlewareFunction } from "react-router";
import { finishCallback, getReturnTo } from "./auth.ts";
import type { CurrentUser } from "./auth.ts";
import { getSessionUser } from "./current-user.ts";

interface AuthContext {
  getUser(): Promise<CurrentUser | null>;
}

type UserLoaderArgs = LoaderFunctionArgs & { user: CurrentUser };
type GuardedLoader<T> = ((args: LoaderFunctionArgs) => Promise<T>) & { hydrate: true };

const authContext = createContext<AuthContext>();
let callbackFailure: { url: string; error: unknown } | undefined;

function documentUrl(value: string | URL) {
  const url = new URL(value);
  url.hash = "";
  return url.href;
}

export const authMiddleware: MiddlewareFunction = async ({ request, context }, next) => {
  if (callbackFailure?.url === documentUrl(request.url)) {
    const { error } = callbackFailure;
    callbackFailure = undefined;
    throw error;
  }
  const result = await finishCallback(request.url);
  if (result) {
    callbackFailure = result.error
      ? { url: documentUrl(result.url), error: result.error }
      : undefined;
    // Restart with a clean router URL before any route loaders or actions run.
    throw replace(result.url);
  }

  let user: Promise<CurrentUser | null> | undefined;
  context.set(authContext, {
    getUser: () => user ??= getSessionUser(),
  });
  return next();
};

export function optionalUser({ context }: Pick<LoaderFunctionArgs, "context">) {
  return context.get(authContext).getUser();
}

export async function requireUser(args: LoaderFunctionArgs) {
  const user = await optionalUser(args);
  if (user) return user;
  const target = new URL(args.request.url);
  target.hash = args.url?.hash ?? "";
  const current = new URL(window.location.href);
  if (!target.hash && documentUrl(target) === documentUrl(current)) target.hash = current.hash;
  const login = new URL("login", new URL(import.meta.env?.BASE_URL ?? "/", target));
  login.searchParams.set("returnTo", getReturnTo(target.href));
  throw replace(login.href);
}

export const requireUserMiddleware: MiddlewareFunction = async (args, next) => {
  await requireUser(args);
  return next();
};

export async function loginLoader(args: LoaderFunctionArgs) {
  const returnTo = getReturnTo(new URL(args.request.url).searchParams.get("returnTo"));
  if (await optionalUser(args)) throw replace(returnTo);
  return { returnTo };
}
loginLoader.hydrate = true as const;

export function withUser(): GuardedLoader<{ user: CurrentUser }>;
export function withUser<T>(load: (args: UserLoaderArgs) => T): GuardedLoader<Awaited<T>>;
export function withUser(load?: (args: UserLoaderArgs) => unknown) {
  async function clientLoader(args: LoaderFunctionArgs) {
    const user = await requireUser(args);
    return load ? load({ ...args, user }) : { user };
  }
  clientLoader.hydrate = true as const;
  return clientLoader;
}
