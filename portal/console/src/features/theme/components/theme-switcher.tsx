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

const options = [
  { value: "light", label: "Clair", icon: SunIcon },
  { value: "dark", label: "Sombre", icon: MoonIcon },
  { value: "system", label: "Système", icon: MonitorIcon },
] as const;

export function ThemeSwitcher() {
  const { preference, setPreference } = useTheme();
  const selected = options.find((option) => option.value === preference);
  const Icon = selected?.icon ?? MonitorIcon;
  const label = `Thème : ${selected?.label ?? "Système"}`;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="icon" aria-label={label} title={label}>
          <Icon aria-hidden="true" strokeWidth={1.6} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>Apparence</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={preference} aria-label="Thème" onValueChange={(value) => {
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
