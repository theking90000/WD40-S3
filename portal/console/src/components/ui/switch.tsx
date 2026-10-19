import * as React from "react"
import { cn } from "~/lib/utils"
import { Switch as SwitchPrimitive } from "radix-ui"

function Switch({
  className,
  size = "default",
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root> & {
  size?: "sm" | "default"
}) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      data-size={size}
      className={cn(
        "peer group/switch inline-flex shrink-0 items-center rounded-full border-0 p-[3px] transition-colors motion-reduce:transition-none outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-45 data-[size=default]:h-[21px] data-[size=default]:w-9 data-[size=sm]:h-3.5 data-[size=sm]:w-6 data-[size=sm]:p-0.5 data-[state=checked]:bg-primary data-[state=unchecked]:bg-input",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className={cn(
          "pointer-events-none block rounded-full bg-switch-thumb shadow-xs ring-0 transition-[background-color,transform] motion-reduce:transition-none group-data-[size=default]/switch:size-[15px] group-data-[size=sm]/switch:size-2.5 data-[state=checked]:bg-primary-foreground data-[state=checked]:translate-x-[15px] data-[state=unchecked]:translate-x-0 group-data-[size=sm]/switch:data-[state=checked]:translate-x-2.5"
        )}
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
