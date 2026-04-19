"use client";

import { TextInput, type TextInputProps } from "flowbite-react";
import { cn } from "@/lib/utils";
import React from "react";

export type InputProps = TextInputProps;

export const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, ...props }, ref) => {
    return (
      <TextInput
        ref={ref}
        className={cn("w-full", className)}
        {...props}
      />
    );
  }
);

Input.displayName = "Input";
