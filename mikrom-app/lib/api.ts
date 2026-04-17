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

function authHeaders(token: string): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Authorization: `Bearer ${token}`,
  };
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
    const result = await response.json();
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
    const result = await response.json();
    if (!response.ok) return { error: result.error || "Login failed" };
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
    const result = await response.json();
    if (!response.ok) return { error: result.error || "Failed to fetch VMs" };
    return { data: result };
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
    const result = await response.json();
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
    const result = await response.json();
    if (!response.ok) return { error: result.error || "Deploy failed" };
    return { data: result };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}
