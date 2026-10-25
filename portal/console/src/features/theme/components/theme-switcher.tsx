import { useTranslation } from "react-i18next";
import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { Button } from "~/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "~/components/ui/dropdown-menu";
import { useTheme } from "../theme";



export function ThemeSwitcher() {
  const { t } = useTranslation();
  const options = [
  { value: "light", label: t($ => $.messages.light), icon: SunIcon },
  { value: "dark", label: t($ => $.messages.dark), icon: MoonIcon },
  { value: "system", label: t($ => $.messages.system), icon: MonitorIcon },
] as const;
  const { preference, setPreference } = useTheme();
  const selected = options.find((option) => option.value === preference);
  const Icon = selected?.icon ?? MonitorIcon;
  const label = t($ => $.messages.themeLabel, { label: selected?.label ?? t($ => $.messages.system) });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="icon" aria-label={label} title={label}>
          <Icon aria-hidden="true" strokeWidth={1.6} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>{t($ => $.messages.appearance)}</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={preference} aria-label={t($ => $.messages.theme)} onValueChange={(value) => {
          if (value === "light" || value === "dark" || value === "system") setPreference(value);
        }}>
          {options.map((option) => (
            <DropdownMenuRadioItem key={option.value} value={option.value} className="text-[11.5px]">
              <option.icon aria-hidden="true" />
              {option.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
