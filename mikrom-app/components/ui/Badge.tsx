import * as React from "react";
import { cn } from "@/lib/utils";

export interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: "default" | "secondary" | "success" | "warning" | "danger" | "outline";
}

function Badge({ className, variant = "default", ...props }: BadgeProps) {
  const variants = {
    default: "bg-zinc-900 text-zinc-50 dark:bg-zinc-50 dark:text-zinc-900",
    secondary: "bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-50",
    success: "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
    warning: "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400",
    danger: "bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400",
    outline: "text-zinc-950 dark:text-zinc-50 border border-zinc-200 dark:border-zinc-800",
  };

  return (
    <div
      className={cn(
        "inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-zinc-500",
        variants[variant],
        className
      )}
      {...props}
    />
  );
}

export { Badge };
