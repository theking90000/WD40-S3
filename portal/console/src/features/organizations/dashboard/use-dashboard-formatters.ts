import { useMemo } from "react";
import { useLocale } from "~/features/i18n/use-locale";
import { createDashboardFormatters } from "./format.ts";

export function useDashboardFormatters() {
  const locale = useLocale();
  return useMemo(() => createDashboardFormatters(locale), [locale]);
}
