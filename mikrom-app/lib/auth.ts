const TOKEN_KEY = "mikrom_token";

export function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  if (typeof window === "undefined") return;
  localStorage.setItem(TOKEN_KEY, token);
}

export function removeToken(): void {
  if (typeof window === "undefined") return;
  localStorage.removeItem(TOKEN_KEY);
}

export function isAuthenticated(): boolean {
  const token = getToken();
  if (!token) return false;
  try {
    const payload = JSON.parse(atob(token.split(".")[1]));
    return typeof payload.exp === "number" && payload.exp * 1000 > Date.now();
  } catch {
    return false;
  }
}

export function logout(): void {
  removeToken();
  if (typeof window !== "undefined") {
    window.location.href = "/auth/login";
  }
}
