import { LanguagesIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "~/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "~/components/ui/dropdown-menu";
import { useLanguage } from "../language-provider";
import { isLanguagePreference, supportedLanguages } from "../preference.ts";

const languageNames = { fr: "Français", en: "English" };

export function LanguageSwitcher() {
  const { preference, language, setPreference } = useLanguage();
  const { t } = useTranslation();
  const languageName = languageNames[language];
  const selectedLabel = preference === "auto"
    ? t($ => $.language.automaticLanguage, { language: languageName })
    : languageName;
  const label = t($ => $.language.trigger, { language: selectedLabel });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="sm" className="h-9 px-2" aria-label={label} title={label}>
          <LanguagesIcon aria-hidden="true" strokeWidth={1.6} />
          <span aria-hidden="true">{language.toUpperCase()}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>{t($ => $.language.label)}</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={preference} aria-label={t($ => $.language.label)} onValueChange={(value) => {
          if (isLanguagePreference(value)) setPreference(value);
        }}>
          <DropdownMenuRadioItem value="auto" className="text-[11.5px]">
            {t($ => $.language.automatic)}
          </DropdownMenuRadioItem>
          {supportedLanguages.map((code) => (
            <DropdownMenuRadioItem key={code} value={code} className="text-[11.5px]">
              <span lang={code}>{languageNames[code]}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
