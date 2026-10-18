import { useEffect, useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { DatabaseIcon, MenuIcon } from "lucide-react";
import { useLocation } from "react-router";
import { Button } from "~/components/ui/button";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "~/components/ui/sheet";
import { TooltipProvider } from "~/components/ui/tooltip";
import { ShellNavigation } from "./navigation";
import type { ShellNavigationGroup } from "./navigation";

interface SidebarContentProps {
  navigation: readonly ShellNavigationGroup[];
  account?: ReactNode;
  onNavigate?: () => void;
}

interface ConsoleShellProps extends Omit<SidebarContentProps, "onNavigate"> {
  title: string;
  organization?: ReactNode;
  organizationName?: string;
  actions?: ReactNode;
  children: ReactNode;
}

function SidebarContent({ navigation, account, onNavigate }: SidebarContentProps) {
  return (
    <>
      <div className="flex items-center gap-2.5 px-1.75 pt-0.75 pb-4.75">
        <span className="flex size-[33px] shrink-0 items-center justify-center rounded-[9px] bg-foreground text-surface">
          <DatabaseIcon aria-hidden="true" className="size-4.5" strokeWidth={1.7} />
        </span>
        <div>
          <p className="font-display text-base leading-none font-bold tracking-tight">WD40-S3</p>
          <p className="mt-1 text-[10px] text-muted-foreground">Cloud console</p>
        </div>
      </div>
      <div className="mt-5.5">
        <ShellNavigation groups={navigation} onNavigate={onNavigate} />
      </div>
      {account && (
        <div className="mt-auto pt-4.5">
          <div className="border-t pt-3">{account}</div>
        </div>
      )}
    </>
  );
}

export function ConsoleShell({
  title,
  navigation,
  organization,
  organizationName,
  account,
  actions,
  children,
}: ConsoleShellProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const contentId = useId();
  const contentRef = useRef<HTMLElement>(null);
  const { pathname } = useLocation();

  useEffect(() => {
    setMenuOpen(false);
    contentRef.current?.scrollTo({ top: 0 });
  }, [pathname]);

  useEffect(() => {
    const desktop = window.matchMedia("(min-width: 961px)");
    function closeOnDesktop(event: MediaQueryListEvent) {
      if (event.matches) setMenuOpen(false);
    }
    desktop.addEventListener("change", closeOnDesktop);
    return () => desktop.removeEventListener("change", closeOnDesktop);
  }, []);

  const sidebar = (
    <SidebarContent
      navigation={navigation}
      account={account}
      onNavigate={() => setMenuOpen(false)}
    />
  );

  return (
    <TooltipProvider>
      <a
        href={`#${contentId}`}
        className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-[60] focus:rounded-lg focus:bg-surface focus:px-4 focus:py-2"
      >
        Aller au contenu
      </a>
      <div className="grid h-dvh grid-cols-1 overflow-hidden bg-background min-[961px]:grid-cols-[252px_minmax(0,1fr)]">
        <aside className="hidden min-h-0 flex-col overflow-y-auto border-r bg-sidebar px-3.5 pt-4.5 pb-3.5 min-[961px]:flex">
          {sidebar}
        </aside>
        <div className="flex min-h-0 min-w-0 flex-col">
          <header className="flex min-h-14 shrink-0 items-center gap-2 border-b px-4 min-[761px]:min-h-15 min-[761px]:gap-4 min-[761px]:px-[clamp(20px,3vw,44px)]">
            <Sheet open={menuOpen} onOpenChange={setMenuOpen}>
              <SheetTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="shrink-0 min-[961px]:hidden"
                  aria-label="Ouvrir la navigation"
                >
                  <MenuIcon aria-hidden="true" strokeWidth={1.6} />
                </Button>
              </SheetTrigger>
              <SheetContent
                side="left"
                aria-describedby={undefined}
                className="max-w-[min(280px,calc(100vw-32px))] bg-sidebar px-3.5 pt-4.5 pb-3.5"
              >
                <SheetTitle className="sr-only">Navigation principale</SheetTitle>
                {sidebar}
              </SheetContent>
            </Sheet>
            {organization}
            <nav aria-label="Fil d'Ariane" className="hidden min-w-0 items-center gap-1.75 text-[11.5px] text-muted-foreground min-[761px]:flex">
              <span>Console</span>
              <span aria-hidden="true">/</span>
              {organizationName && (
                <>
                  <span className="hidden truncate min-[961px]:inline">{organizationName}</span>
                  <span aria-hidden="true" className="hidden min-[961px]:inline">/</span>
                </>
              )}
              <span aria-current="page" className="truncate font-semibold text-foreground">{title}</span>
            </nav>
            {actions && (
              <div className="ml-auto flex min-w-0 flex-1 items-center justify-end gap-1.75">
                {actions}
              </div>
            )}
          </header>
          <main
            id={contentId}
            ref={contentRef}
            tabIndex={-1}
            className="m-0 w-full max-w-none min-w-0 flex-1 overflow-y-auto p-4 pt-5.5 pb-12 min-[621px]:px-[clamp(20px,3vw,44px)] min-[621px]:pt-7.5 min-[621px]:pb-16"
          >
            <div className="mx-auto w-full max-w-[1260px]">{children}</div>
          </main>
        </div>
      </div>
    </TooltipProvider>
  );
}
