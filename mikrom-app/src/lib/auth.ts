const TOKEN_KEY = "mikrom_token";

function browserAvailable() {
  return typeof window !== "undefined";
}

function decodePayload(token: string): { exp?: number } | null {
  try {
    return JSON.parse(atob(token.split(".")[1]));
  } catch {
    return null;
  }
}

export function getToken(): string | null {
  if (!browserAvailable()) return null;
  const token = localStorage.getItem(TOKEN_KEY);
  if (!token) return null;

  const payload = decodePayload(token);
  if (payload?.exp && payload.exp * 1000 <= Date.now()) {
    localStorage.removeItem(TOKEN_KEY);
    return null;
  }

  return token;
}

export function setToken(token: string) {
  if (!browserAvailable()) return;
  localStorage.setItem(TOKEN_KEY, token);
  window.dispatchEvent(new Event("mikrom-auth-change"));
}

export function removeToken() {
  if (!browserAvailable()) return;
  localStorage.removeItem(TOKEN_KEY);
  window.dispatchEvent(new Event("mikrom-auth-change"));
}

export function isAuthenticated() {
  return Boolean(getToken());
}

export function logout() {
  removeToken();
  if (browserAvailable()) {
    window.location.href = "/auth/login";
  }
}
