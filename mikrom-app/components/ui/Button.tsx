"use client";

import { Button as FlowbiteButton, type ButtonProps as FlowbiteButtonProps } from "flowbite-react";
import { cn } from "@/lib/utils";
import React from "react";

export interface ButtonProps extends FlowbiteButtonProps {
  variant?: "default" | "outline" | "ghost" | "secondary" | "danger" | "link";
}

export const Button = React.forwardRef<HTMLButtonElement | HTMLAnchorElement, ButtonProps>(
  ({ className, variant = "default", color, ...props }, ref) => {
    // Map existing variants to Flowbite colors/styles
    let flowbiteColor = color;
    
    if (!color) {
      switch (variant) {
        case "default":
          flowbiteColor = "dark";
          break;
        case "outline":
          flowbiteColor = "gray";
          break;
        case "ghost":
          flowbiteColor = "gray";
          break;
        case "secondary":
          flowbiteColor = "light";
          break;
        case "danger":
          flowbiteColor = "failure";
          break;
        default:
          flowbiteColor = "dark";
      }
    }

    return (
      <FlowbiteButton
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ref={ref as any}
        color={flowbiteColor}
        outline={variant === "outline"}
        className={cn(
          variant === "ghost" && "bg-transparent hover:bg-zinc-100 dark:hover:bg-zinc-800 border-none text-zinc-600 dark:text-zinc-400",
          className
        )}
        {...props}
      />
    );
  }
);

Button.displayName = "Button";
