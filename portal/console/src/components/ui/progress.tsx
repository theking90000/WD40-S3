"use client"

import * as React from "react"
import { cn } from "~/lib/utils"
import { Progress as ProgressPrimitive } from "radix-ui"

function Progress({
  className,
  value,
  max = 100,
  ...props
}: React.ComponentProps<typeof ProgressPrimitive.Root>) {
  const limit = Number.isFinite(max) && max > 0 ? max : 100
  const progress = value == null || !Number.isFinite(value)
    ? null
    : Math.min(Math.max(value, 0), limit)

  return (
    <ProgressPrimitive.Root
      data-slot="progress"
      value={progress}
      max={limit}
      className={cn(
        "relative h-[5px] w-full overflow-hidden rounded-full bg-foreground-soft",
        className
      )}
      {...props}
    >
      <ProgressPrimitive.Indicator
        data-slot="progress-indicator"
        className="h-full w-full rounded-full bg-primary transition-transform data-[state=indeterminate]:w-1/3 motion-safe:data-[state=indeterminate]:animate-pulse motion-reduce:transition-none"
        style={{ transform: progress === null ? undefined : `translateX(-${100 - (progress / limit) * 100}%)` }}
      />
    </ProgressPrimitive.Root>
  )
}

export { Progress }
