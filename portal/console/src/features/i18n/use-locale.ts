import { useLanguage } from "./language-provider";

export function useLocale() {
  const { language } = useLanguage();
  return language === "fr" ? "fr-BE" : "en-GB";
}
