import { createContext, redirectDocument, replace } from "react-router";
import { finishCallback, getCurrentUser, getLoginUrl } from "./auth.js";

const authContext = createContext();
let callbackFailure;

function documentUrl(value) {
  const url = new URL(value);
  url.hash = "";
  return url.href;
}

export async function authMiddleware({ request, context }, next) {
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

  let user;
  let login;
  context.set(authContext, {
    getUser: () => user ??= getCurrentUser({ signal: request.signal }),
    getLogin: (returnTo) => login ??= getLoginUrl(returnTo, { signal: request.signal }),
  });
  return next();
}

export function optionalUser({ context }) {
  return context.get(authContext).getUser();
}

export async function requireUser(args) {
  const user = await optionalUser(args);
  if (user) return user;
  const target = new URL(args.url?.href ?? args.request.url);
  const current = new URL(window.location.href);
  if (documentUrl(target) === documentUrl(current)) target.hash = current.hash;
  throw redirectDocument(await args.context.get(authContext).getLogin(target.href));
}

export function withUser(load = ({ user }) => ({ user })) {
  async function clientLoader(args) {
    const user = await requireUser(args);
    return load({ ...args, user });
  }
  clientLoader.hydrate = true;
  return clientLoader;
}
