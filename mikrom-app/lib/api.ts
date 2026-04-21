import { logout } from "@/lib/auth";

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

export interface UserProfile {
  id: string;
  email: string;
  role: string;
  first_name: string | null;
  last_name: string | null;
}

export interface UpdateProfileRequest {
  first_name: string | null;
  last_name: string | null;
}

export interface VmInfo {
  job_id: string;
  app_id: string;
  app_name: string;
  image: string;
  status: string;
  host_id: string;
  vm_id: string;
}
export interface VmStatus {
  job_id: string;
  status: string;
  host_id: string;
  vm_id: string;
  scheduled_at: number;
  started_at: number;
  stopped_at: number;
  error_message: string;
  cpu_usage: number;
  ram_used_bytes: number;
}

export interface LogLine {
  line: string;
  timestamp: number;
}

// ... other exports ...

export function getVmLogsSSE(
  token: string,
  jobId: string,
  onMessage: (log: LogLine) => void,
  onError: (err: string) => void
): () => void {
  const abortController = new AbortController();
  let isAborted = false;

  const connect = async () => {
    while (!isAborted) {
      try {
        const response = await fetch(`${API_BASE_URL}/vms/${jobId}/logs?follow=true`, {
          headers: {
            Authorization: `Bearer ${token}`,
          },
          signal: abortController.signal,
        });

        if (!response.ok) {
          if (response.status === 401) logout();
          throw new Error(`Failed to connect to log stream: ${response.statusText}`);
        }

        const reader = response.body?.getReader();
        if (!reader) {
          throw new Error("No response body");
        }

        const decoder = new TextDecoder();
        let buffer = "";

        while (!isAborted) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split("\n\n");
          buffer = lines.pop() || "";

          for (const line of lines) {
            if (line.startsWith("data: ")) {
              try {
                const data = JSON.parse(line.slice(6));
                onMessage(data as LogLine);
              } catch (err) {
                console.error("Failed to parse log line", err);
              }
            }
          }
        }
      } catch (err) {
        if (err instanceof Error && err.name === "AbortError") {
          isAborted = true;
          return;
        }
        console.error("SSE Fetch Error", err);
        onError(err instanceof Error ? err.message : "Connection lost to log stream");
        
        // Wait before reconnecting
        if (!isAborted) {
          await new Promise(resolve => setTimeout(resolve, 2000));
        }
      }
    }
  };

  connect();

  return () => {
    isAborted = true;
    abortController.abort();
  };
}

export interface DeployRequest {
  app_name: string;
  image: string;
  vcpus?: number;
  memory_mib?: number;
  disk_mib?: number;
}

export interface DeployResponse {
  job_id: string;
  status: string;
  host_id: string | null;
  vm_id: string | null;
  message: string;
}

export interface StopVmResponse {
  success: boolean;
  message: string;
}

function authHeaders(token: string): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Authorization: `Bearer ${token}`,
  };
}

async function parseJson<T>(response: Response): Promise<T> {
  if (response.status === 401) logout();
  const text = await response.text();
  if (!text) throw new Error("Empty response from server");
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new Error(`API unavailable (${response.status})`);
  }
}

export async function register(
  data: RegisterRequest
): Promise<{ data?: RegisterResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    const result = await parseJson<RegisterResponse & ApiError>(response);
    if (!response.ok) return { error: result.error || "Registration failed" };
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
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    const result = await parseJson<LoginResponse & ApiError>(response);
    if (!response.ok) return { error: result.error || "Login failed" };
    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function getUserProfile(
  token: string
): Promise<{ data?: UserProfile; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/me`, {
      headers: authHeaders(token),
    });
    const result = await parseJson<UserProfile & ApiError>(response);
    if (!response.ok) return { error: result.error || "Failed to fetch profile" };
    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function updateUserProfile(
  token: string,
  data: UpdateProfileRequest
): Promise<{ data?: UserProfile; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/me`, {
      method: "PUT",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<UserProfile & ApiError>(response);
    if (!response.ok) return { error: result.error || "Failed to update profile" };
    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function listVms(
  token: string
): Promise<{ data?: VmInfo[]; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/vms`, {
      headers: authHeaders(token),
    });
    const result = await parseJson<VmInfo[] & ApiError>(response);
    if (!response.ok) return { error: (result as ApiError).error || "Failed to fetch VMs" };
    return { data: result as VmInfo[] };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function getVm(
  token: string,
  jobId: string
): Promise<{ data?: VmStatus; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/vms/${jobId}`, {
      headers: authHeaders(token),
    });
    const result = await parseJson<VmStatus & ApiError>(response);
    if (!response.ok) return { error: result.error || "VM not found" };
    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function deployApp(
  token: string,
  data: DeployRequest
): Promise<{ data?: DeployResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deploy`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<DeployResponse & ApiError>(response);
    if (!response.ok) return { error: result.error || "Deploy failed" };
    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function stopVm(
  token: string,
  jobId: string
): Promise<{ data?: StopVmResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/vms/${jobId}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    const result = await parseJson<StopVmResponse & ApiError>(response);
    if (!response.ok) return { error: (result as ApiError).error || "Failed to stop VM" };
    return { data: result as StopVmResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function pauseVm(
  token: string,
  jobId: string
): Promise<{ data?: StopVmResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/vms/${jobId}/pause`, {
      method: "POST",
      headers: authHeaders(token),
    });
    const result = await parseJson<StopVmResponse & ApiError>(response);
    if (!response.ok) return { error: (result as ApiError).error || "Failed to pause VM" };
    return { data: result as StopVmResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function resumeVm(
  token: string,
  jobId: string
): Promise<{ data?: StopVmResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/vms/${jobId}/resume`, {
      method: "POST",
      headers: authHeaders(token),
    });
    const result = await parseJson<StopVmResponse & ApiError>(response);
    if (!response.ok) return { error: (result as ApiError).error || "Failed to resume VM" };
    return { data: result as StopVmResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function deleteVm(
  token: string,
  jobId: string
): Promise<{ data?: StopVmResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/vms/${jobId}/delete`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    const result = await parseJson<StopVmResponse & ApiError>(response);
    if (!response.ok) return { error: (result as ApiError).error || "Failed to delete VM" };
    return { data: result as StopVmResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}
