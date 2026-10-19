import { useState } from "react";
import { Building2Icon, CheckIcon, ChevronsUpDownIcon, PlusIcon } from "lucide-react";
import { Link } from "react-router";
import { useOrganization, useOrganizations } from "~/features/organizations/router.ts";
import { Button } from "~/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "~/components/ui/dropdown-menu";
import { CreateOrganizationDialog } from "./create-organization";

export function OrganizationSwitcher() {
  const [creating, setCreating] = useState(false);
  const { user, data = [] } = useOrganizations();
  const organization = useOrganization();

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="outline" className="w-full min-w-0 justify-start" aria-label={`Organisation : ${organization?.name ?? "Choisir une organisation"}`}>
            <Building2Icon aria-hidden="true" className="size-4 text-brand" />
            <span className="min-w-0 flex-1 truncate text-left">{organization?.name ?? "Choisir une organisation"}</span>
            <ChevronsUpDownIcon aria-hidden="true" className="size-3.5 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-72">
          <DropdownMenuLabel>Vos organisations</DropdownMenuLabel>
          {data.map((item) => (
            <DropdownMenuItem key={item.id} asChild>
              <Link to={`/${item.id}/`} aria-current={item.id === organization?.id ? "true" : undefined}>
                <span className="min-w-0 flex-1 truncate">{item.name}</span>
                {item.id === organization?.id && <CheckIcon aria-hidden="true" className="size-3.5" />}
              </Link>
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setCreating(true)}>
            <PlusIcon aria-hidden="true" />Créer une organisation
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <CreateOrganizationDialog user={user} open={creating} onOpenChange={setCreating} />
    </>
  );
}
