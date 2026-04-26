"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { health, API_BASE_URL, HealthResponse } from "@/lib/api";

export function useHealth() {
  const queryClient = useQueryClient();
  const queryKey = ["health"];

  // Polling as a base (or fallback)
  const query = useQuery({
    queryKey,
    queryFn: async () => {
      return await health();
    },
    refetchInterval: 30000, // Background polling every 30s as a fallback
  });

  // Real-time updates via SSE
  useEffect(() => {
    const eventSource = new EventSource(`${API_BASE_URL}/health/stream`);

    eventSource.onmessage = (event) => {
      try {
        const data: HealthResponse = JSON.parse(event.data);
        queryClient.setQueryData(["health"], data);
      } catch (err) {
        console.error("Error parsing health SSE data:", err);
      }
    };

    eventSource.onerror = (err) => {
      console.error("Health SSE error:", err);
    };

    return () => {
      eventSource.close();
    };
  }, [queryClient]);

  return query;
}
