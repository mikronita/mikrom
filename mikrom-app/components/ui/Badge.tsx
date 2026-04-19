"use client";

import { Badge as FlowbiteBadge, type BadgeProps as FlowbiteBadgeProps } from "flowbite-react";
import { cn } from "@/lib/utils";
import React from "react";

export interface BadgeProps extends FlowbiteBadgeProps {
  variant?: "default" | "secondary" | "outline" | "destructive" | "success" | "warning";
}

export const Badge = ({ className, variant = "default", color, ...props }: BadgeProps) => {
  let flowbiteColor = color;

  if (!color) {
    switch (variant) {
      case "default":
        flowbiteColor = "dark";
        break;
      case "secondary":
        flowbiteColor = "light";
        break;
      case "success":
        flowbiteColor = "success";
        break;
      case "warning":
        flowbiteColor = "warning";
        break;
      case "destructive":
        flowbiteColor = "failure";
        break;
      case "outline":
        flowbiteColor = "gray";
        break;
      default:
        flowbiteColor = "info";
    }
  }

  return (
    <FlowbiteBadge
      color={flowbiteColor}
      className={cn(
        "font-semibold",
        variant === "outline" && "bg-transparent border border-zinc-200 dark:border-zinc-800",
        className
      )}
      {...props}
    />
  );
};
