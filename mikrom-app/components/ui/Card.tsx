"use client";

import { Card as FlowbiteCard } from "flowbite-react";
import { cn } from "@/lib/utils";
import React from "react";

interface CardProps extends React.ComponentProps<typeof FlowbiteCard> {
  noPadding?: boolean;
}

export const Card = ({ className, children, noPadding = false, ...props }: CardProps) => (
  <FlowbiteCard 
    className={cn(
      "overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-sm dark:border-zinc-800 dark:bg-zinc-900",
      className
    )} 
    {...props}
  >
    <div className={cn("flex h-full flex-col justify-center gap-4", !noPadding && "p-6")}>
      {children}
    </div>
  </FlowbiteCard>
);

export const CardHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("flex flex-col space-y-1.5 p-0 pb-4", className)} {...props} />
);

export const CardTitle = ({ className, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
  <h3 className={cn("text-2xl font-semibold leading-none tracking-tight dark:text-white", className)} {...props} />
);

export const CardDescription = ({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => (
  <p className={cn("text-sm text-zinc-500 dark:text-zinc-400", className)} {...props} />
);

export const CardContent = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("p-0 pt-0", className)} {...props} />
);

export const CardFooter = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("flex items-center p-0 pt-4", className)} {...props} />
);
