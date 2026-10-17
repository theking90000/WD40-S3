import { createContext, redirectDocument, replace } from "react-router";
import type { LoaderFunctionArgs, MiddlewareFunction } from "react-router";
import { finishCallback, getCurrentUser, getLoginUrl } from "./auth.ts";
import type { CurrentUser } from "./auth.ts";

interface AuthContext {
  getUser(): Promise<CurrentUser | null>;
  getLogin(returnTo: string): Promise<string>;
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
  let login: Promise<string> | undefined;
  context.set(authContext, {
    getUser: () => user ??= getCurrentUser({ signal: request.signal }),
    getLogin: (returnTo) => login ??= getLoginUrl(returnTo, { signal: request.signal }),
  });
  return next();
};

export function optionalUser({ context }: Pick<LoaderFunctionArgs, "context">) {
  return context.get(authContext).getUser();
}

export async function requireUser(args: LoaderFunctionArgs) {
  const user = await optionalUser(args);
  if (user) return user;
  const target = new URL(args.url?.href ?? args.request.url);
  const current = new URL(window.location.href);
  if (documentUrl(target) === documentUrl(current)) target.hash = current.hash;
  throw redirectDocument(await args.context.get(authContext).getLogin(target.href));
}

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
