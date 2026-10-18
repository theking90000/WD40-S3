import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { MenuIcon } from "lucide-react";
import { useLocation } from "react-router";
import { Button } from "~/components/ui/button";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "~/components/ui/sheet";

export function MobileNavigation({ children }: { children: (close: () => void) => ReactNode }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const { pathname } = useLocation();

  useEffect(() => {
    setMenuOpen(false);
  }, [pathname]);

  useEffect(() => {
    const desktop = window.matchMedia("(min-width: 961px)");
    function closeOnDesktop(event: MediaQueryListEvent) {
      if (event.matches) setMenuOpen(false);
    }
    desktop.addEventListener("change", closeOnDesktop);
    return () => desktop.removeEventListener("change", closeOnDesktop);
  }, []);

  return (
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
        {children(() => setMenuOpen(false))}
      </SheetContent>
    </Sheet>
  );
}
