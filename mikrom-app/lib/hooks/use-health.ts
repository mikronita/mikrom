"use client";

import { useQuery } from "@tanstack/react-query";
import { health } from "@/lib/api";

export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: async () => {
      return await health();
    },
    refetchInterval: 30000, // Refetch every 30 seconds
  });
}
