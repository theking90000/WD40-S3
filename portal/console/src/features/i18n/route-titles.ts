import titles from "../../locales/fr/route-titles.ts";

export type RouteTitleKey = keyof typeof titles;

export function routeTitle(titleKey: RouteTitleKey) {
  return { titleKey };
}

export function isRouteTitleHandle(handle: unknown): handle is ReturnType<typeof routeTitle> {
  return !!handle && typeof handle === "object" && "titleKey" in handle
    && typeof handle.titleKey === "string" && Object.hasOwn(titles, handle.titleKey);
}
