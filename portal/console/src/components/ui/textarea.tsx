import * as React from "react"
import { cn } from "~/lib/utils"

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "flex min-h-[82px] w-full resize-y rounded-lg border border-input bg-surface px-[11px] py-2 text-xs leading-normal text-foreground transition-[border-color,box-shadow] motion-reduce:transition-none outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-45 aria-invalid:border-destructive aria-invalid:ring-[3px] aria-invalid:ring-destructive/20 aria-invalid:focus-visible:ring-destructive",
        className
      )}
      {...props}
    />
  )
}

export { Textarea }
