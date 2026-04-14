const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || "http://localhost:5001";

export interface RegisterRequest {
  email: string;
  password: string;
}

export interface RegisterResponse {
  message: string;
  user_id: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface LoginResponse {
  token: string;
}

export interface ApiError {
  error: string;
}

export async function register(
  data: RegisterRequest
): Promise<{ data?: RegisterResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/register`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(data),
    });

    const result = await response.json();

    if (!response.ok) {
      return { error: result.error || "Registration failed" };
    }

    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function login(
  data: LoginRequest
): Promise<{ data?: LoginResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/login`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(data),
    });

    const result = await response.json();

    if (!response.ok) {
      return { error: result.error || "Login failed" };
    }

    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}
