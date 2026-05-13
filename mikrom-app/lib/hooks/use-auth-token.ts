"use client";

import { useEffect, useState } from "react";

import { getToken } from "@/lib/auth";

const AUTH_EVENT = "mikrom-auth-change";

export function useAuthToken(): string | null {
  const [token, setTokenState] = useState<string | null>(() => getToken());

  useEffect(() => {
    if (typeof window === "undefined") return;

    const syncToken = () => setTokenState(getToken());
    window.addEventListener("storage", syncToken);
    window.addEventListener(AUTH_EVENT, syncToken);
    syncToken();

    return () => {
      window.removeEventListener("storage", syncToken);
      window.removeEventListener(AUTH_EVENT, syncToken);
    };
  }, []);

  return token;
}
