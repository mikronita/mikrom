import { logout } from "@/lib/auth";

export const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || "http://localhost:5001";

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

export interface HealthResponse {
  status: string;
  version: string;
}

export interface LiveDeploymentInfo {
  job_id: string;
  deployment_id: string;
  app_id: string;
  app_name: string;
  image: string;
  status: string;
  host_id: string;
  vm_id: string;
  vcpus?: number;
  memory_mib?: number;
  cpu_usage?: number;
  ram_used_bytes?: number;
}

export interface LiveDeploymentStatus {
  job_id: string;
  deployment_id?: string;
  app_id?: string;
  app_name?: string;
  image?: string;
  status: string;
  host_id: string;
  vm_id: string;
  scheduled_at: number;
  started_at: number;
  stopped_at: number;
  error_message: string;
  cpu_usage: number;
  ram_used_bytes: number;
  vcpus?: number;
  memory_mib?: number;
}

export interface LogLine {
  line: string;
  timestamp: number;
}

export interface VmMetrics {
  cpu_usage: number;
  memory_usage: number;
  disk_usage: number;
  network_rx: number;
  network_tx: number;
}

export interface VmMetricsResponse {
  job_id: string;
  metrics: VmMetrics;
}

export interface PauseDeploymentResponse {
  success: boolean;
  message: string;
}

export interface ResumeDeploymentResponse {
  success: boolean;
  message: string;
}

export interface StopDeploymentResponse {
  success: boolean;
  message: string;
}

export interface AppInfo {
  id: string;
  name: string;
  git_url: string;
  port: number;
  hostname: string | null;
  github_webhook_secret?: string;
  active_deployment_id: string | null;
  created_at: string;
}

export interface CreateAppRequest {
  name: string;
  git_url: string;
}

export interface DeploymentInfo {
  id: string;
  app_id: string;
  build_id: string | null;
  image_tag: string | null;
  job_id: string | null;
  status: string;
  git_commit_hash: string | null;
  git_commit_message: string | null;
  git_branch: string | null;
  trigger_source: string;
  created_at: string;
  updated_at: string;
}

export interface DeployRequest {
  app_name: string;
  image: string;
  git_url?: string;
  port?: number;
  vcpus?: number;
  memory_mib?: number;
  disk_mib?: number;
  env?: Record<string, string>;
}

export interface DeployResponse {
  job_id?: string;
  deployment_id?: string;
  status: string;
  host_id?: string;
  vm_id?: string;
  image_tag?: string;
  message: string;
}

const authHeaders = (token: string) => ({
  "Content-Type": "application/json",
  Authorization: `Bearer ${token}`,
});

async function parseJson<T>(response: Response): Promise<T | ApiError> {
  try {
    return await response.json();
  } catch {
    return { error: "Invalid JSON response from server" };
  }
}

function getErrorMessage(result: unknown, fallback: string): string {
  if (
    result !== null &&
    typeof result === "object" &&
    "error" in result &&
    typeof (result as Record<string, unknown>).error === "string"
  ) {
    return (result as { error: string }).error;
  }
  return fallback;
}

export async function health(): Promise<HealthResponse> {
  const response = await fetch(`${API_BASE_URL}/health`);
  return response.json();
}

export async function register(data: RegisterRequest): Promise<{ data?: RegisterResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    const result = await parseJson<RegisterResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Registration failed") };
    return { data: result as RegisterResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function login(data: LoginRequest): Promise<{ data?: LoginResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    const result = await parseJson<LoginResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Login failed") };
    return { data: result as LoginResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function getProfile(token: string): Promise<{ data?: UserProfile; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/me`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<UserProfile>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch profile") };
    return { data: result as UserProfile };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export const getUserProfile = getProfile;

export async function updateProfile(token: string, data: UpdateProfileRequest): Promise<{ data?: UserProfile; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/auth/me`, {
      method: "PUT",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<UserProfile>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to update profile") };
    return { data: result as UserProfile };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export const updateUserProfile = updateProfile;

export async function listActiveDeployments(token: string): Promise<{ data?: LiveDeploymentInfo[]; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/active`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<LiveDeploymentInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch active deployments") };
    return { data: result as LiveDeploymentInfo[] };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function getLiveDeploymentStatus(token: string, jobId: string): Promise<{ data?: LiveDeploymentStatus; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/${jobId}`, {
      headers: authHeaders(token),
    });
    const result = await parseJson<LiveDeploymentStatus>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch deployment status") };
    return { data: result as LiveDeploymentStatus };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export function getDeploymentLogsSSE(
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
        const response = await fetch(`${API_BASE_URL}/deployments/${jobId}/logs?follow=true`, {
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
          const { value, done } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split("\n\n");
          buffer = lines.pop() || "";

          for (const line of lines) {
            if (line.startsWith("data: ")) {
              try {
                const data = JSON.parse(line.substring(6));
                onMessage(data);
              } catch (e) {
                console.error("Failed to parse log line", e);
              }
            }
          }
        }
      } catch (err) {
        if (!isAborted) {
          const message = err instanceof Error ? err.message : "Stream error";
          console.error("SSE Connection Error:", err);
          onError(message);
          // Wait before reconnecting
          await new Promise((resolve) => setTimeout(resolve, 3000));
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

export function watchDeploymentsSSE(
  token: string,
  onMessage: (deployment: LiveDeploymentInfo) => void
): () => void {
  const eventSource = new EventSource(`${API_BASE_URL}/deployments/events?token=${token}`);

  eventSource.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onMessage(data);
    } catch (e) {
      console.error("Failed to parse deployment event", e);
    }
  };

  eventSource.onerror = () => {
    console.debug("VMs SSE connection error, attempting to reconnect...");
  };

  return () => {
    eventSource.onmessage = null;
    eventSource.onerror = null;
    eventSource.close();
  };
}

export async function pauseDeployment(token: string, jobId: string): Promise<{ data?: PauseDeploymentResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/${jobId}/pause`, {
      method: "POST",
      headers: authHeaders(token),
    });
    const result = await parseJson<PauseDeploymentResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to pause deployment") };
    return { data: result as PauseDeploymentResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function resumeDeployment(token: string, jobId: string): Promise<{ data?: ResumeDeploymentResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/${jobId}/resume`, {
      method: "POST",
      headers: authHeaders(token),
    });
    const result = await parseJson<ResumeDeploymentResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to resume deployment") };
    return { data: result as ResumeDeploymentResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function stopDeployment(token: string, jobId: string): Promise<{ data?: StopDeploymentResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/${jobId}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    const result = await parseJson<StopDeploymentResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to stop deployment") };
    return { data: result as StopDeploymentResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function deleteDeploymentRecord(token: string, jobId: string): Promise<{ success: boolean; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/${jobId}/delete`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete deployment record") };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function getVmMetrics(token: string, jobId: string): Promise<{ data?: VmMetricsResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/deployments/${jobId}/metrics`, {
      headers: authHeaders(token),
    });
    const result = await parseJson<VmMetricsResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch metrics") };
    return { data: result as VmMetricsResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function listApps(token: string): Promise<{ data?: AppInfo[]; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/apps`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<AppInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch apps") };
    return { data: result as AppInfo[] };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function createApp(token: string, data: CreateAppRequest): Promise<{ data?: AppInfo; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/apps`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<AppInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create app") };
    return { data: result as AppInfo };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function deleteApp(token: string, appName: string): Promise<{ success: boolean; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/apps/${appName}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete app") };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : "Network error" };
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
    const result = await parseJson<DeployResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to start deployment") };
    return { data: result as DeployResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function deployAppVersion(
  token: string,
  appName: string,
  data: Partial<DeployRequest> = {}
): Promise<{ data?: DeployResponse; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/apps/${appName}/deploy`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<DeployResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to start deployment") };
    return { data: result as DeployResponse };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function listDeployments(
  token: string,
  appName: string
): Promise<{ data?: DeploymentInfo[]; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/apps/${appName}/deployments`, {
      headers: authHeaders(token),
    });
    const result = await parseJson<DeploymentInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch deployments") };
    return { data: result as DeploymentInfo[] };
  } catch (err) {
    return { error: err instanceof Error ? err.message : "Network error" };
  }
}

export async function activateDeployment(
  token: string,
  appName: string,
  deploymentId: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const response = await fetch(`${API_BASE_URL}/apps/${appName}/deployments/${deploymentId}/activate`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to activate deployment") };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : "Network error" };
  }
}

// Transitional aliases
export const listVms = listActiveDeployments;
export const watchVmsSSE = watchDeploymentsSSE;
export const getVmStatus = getLiveDeploymentStatus;
export const getVm = getVmStatus;
export const getVmLogsSSE = getDeploymentLogsSSE;
export const pauseVm = pauseDeployment;
export const resumeVm = resumeDeployment;
export const stopVm = stopDeployment;
export const deleteVm = deleteDeploymentRecord;
