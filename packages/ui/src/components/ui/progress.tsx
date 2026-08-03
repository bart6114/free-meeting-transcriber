import * as React from "react";

import { cn } from "@hypr/utils";

interface ProgressProps extends React.HTMLAttributes<HTMLDivElement> {
  value: number;
}

const Progress = React.forwardRef<HTMLDivElement, ProgressProps>(
  ({ value, className, ...props }, ref) => {
    const percentage = Math.min(100, Math.max(0, value * 100));

    return (
      <div
        ref={ref}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(percentage)}
        className={cn([
          "bg-accent relative h-1 w-full overflow-hidden rounded-full",
          className,
        ])}
        {...props}
      >
        <div
          className="bg-primary h-full rounded-full transition-[width] duration-300"
          style={{ width: `${percentage}%` }}
        />
      </div>
    );
  },
);

Progress.displayName = "Progress";

export { Progress };
