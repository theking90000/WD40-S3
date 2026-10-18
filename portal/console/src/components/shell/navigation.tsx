import type { LucideIcon } from "lucide-react";
import { NavLink } from "react-router";
import { cn } from "~/lib/utils";

interface ShellNavigationItem {
  label: string;
  to: string;
  icon: LucideIcon;
  end?: boolean;
  count?: number;
}

export interface ShellNavigationGroup {
  label: string;
  items: readonly ShellNavigationItem[];
}

interface ShellNavigationProps {
  groups: readonly ShellNavigationGroup[];
  onNavigate?: () => void;
}

export function ShellNavigation({ groups, onNavigate }: ShellNavigationProps) {
  return (
    <nav aria-label="Navigation principale" className="flex flex-col gap-4.5">
      {groups.map((group) => (
        <div key={group.label} className="flex flex-col gap-0.5">
          <p className="px-2.5 pb-1.5 text-[10.5px] font-semibold text-muted-foreground">
            {group.label}
          </p>
          {group.items.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              onClick={onNavigate}
              className={({ isActive }) => cn(
                "relative flex min-h-9.5 items-center gap-2.5 rounded-lg px-2.5 py-2 text-[12.5px] transition-colors motion-reduce:transition-none",
                isActive
                  ? "bg-surface font-semibold text-foreground shadow-[0_0_0_1px_var(--border)] before:absolute before:inset-y-[9px] before:left-0 before:w-0.5 before:rounded-full before:bg-brand"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <item.icon aria-hidden="true" className="size-4 shrink-0 opacity-85" strokeWidth={1.6} />
              <span className="min-w-0 truncate">{item.label}</span>
              {item.count !== undefined && (
                <span className="ml-auto rounded-full bg-foreground-soft px-1.75 py-px text-[10px] font-semibold text-muted-foreground tabular-nums">
                  {item.count}
                </span>
              )}
            </NavLink>
          ))}
        </div>
      ))}
    </nav>
  );
}
