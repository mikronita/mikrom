import { browser } from "$app/environment";
import { env } from "$env/dynamic/public";
import { logout } from "$lib/auth";
import { createFetchSseStream } from "$lib/utils/sse";

const PUBLIC_BASE = (
  browser ? window.location.origin : env.PUBLIC_APP_URL || "https://mikrom.spluca.org"
)
  .replace("http://[::1]", "http://localhost")
  .replace("https://[::1]", "https://localhost")
  .replace("http://[0:0:0:0:0:0:0:1]", "http://localhost")
  .replace("https://[0:0:0:0:0:0:0:1]", "https://localhost")
  .replace(/\/+$/, "");

const API_PROXY_BASE = "/api/v1";
export const API_BASE_URL = `${PUBLIC_BASE}${API_PROXY_BASE}`;

export function resolveAvatarUrl(avatarUrl: string | null | undefined) {
  if (!avatarUrl) return null;
  if (/^https?:\/\//i.test(avatarUrl)) return avatarUrl;
  return `${API_BASE_URL}${avatarUrl.startsWith("/") ? avatarUrl : `/${avatarUrl}`}`;
}

export interface ApiError {
  error: string;
}

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
  code?: string;
}

export interface LoginResponse {
  token?: string;
  requires_2fa?: boolean;
}

export interface UserProfile {
  id: string;
  email: string;
  role: string;
  first_name: string | null;
  last_name: string | null;
  avatar_url: string | null;
  vpc_ipv6_prefix: string | null;
  totp_enabled?: boolean;
  email_notifications?: boolean;
  marketing_emails?: boolean;
}

export interface ProjectInfo {
  id: string;
  tenant_id: string;
  name: string;
  created_at: string;
  updated_at?: string;
}

export interface CreateProjectRequest {
  name: string;
}

export interface UpdateProjectRequest {
  name: string;
}

export type DatabaseApiStatus = "pending" | "running" | "failed" | "deleting";

export interface DatabaseInfo {
  id: string;
  name: string;
  engine: string;
  postgres_version: number;
  neon_tenant_id?: string | null;
  neon_timeline_id?: string | null;
  tenant_gen?: number | null;
  status: DatabaseApiStatus;
  vcpus: number;
  memory_mib: number;
  disk_mib: number;
  created_at: string;
  updated_at: string;
}

export interface CreateDatabaseRequest {
  name: string;
  engine: string;
  postgres_version?: number;
  vcpus?: number;
  memory_mib?: number;
  disk_mib?: number;
  settings?: Record<string, string>;
}

export interface DatabaseConnectionInfo {
  database_id: string;
  database_name: string;
  database_user: string;
  database_host: string;
  database_port: number;
  ssh_host: string;
  ssh_user: string;
  ssh_port: number;
  ssh_tunnel_command: string;
  psql_command: string;
}

export interface DatabaseBranchInfo {
  database_id: string;
  database_name: string;
  branch_name: string;
  neon_tenant_id: string | null;
  neon_timeline_id: string | null;
  tenant_gen: number | null;
  status: DatabaseApiStatus;
  is_current: boolean;
  created_at: string;
  updated_at: string;
}

export interface DatabaseBackupInfo {
  database_id: string;
  database_name: string;
  backup_strategy: string;
  recovery_mode: string;
  retention_valid: boolean;
  neon_tenant_id: string | null;
  neon_timeline_id: string | null;
  tenant_gen: number | null;
  status: DatabaseApiStatus;
  created_at: string;
  updated_at: string;
}

export interface DatabaseSnapshotInfo {
  id: string;
  name: string;
  created_at: number;
  size_bytes: number;
  vm_status: string;
}

export interface DatabaseSnapshotListResponse {
  success: boolean;
  message: string;
  snapshots: DatabaseSnapshotInfo[];
}

export interface DatabaseSnapshotActionResponse {
  success: boolean;
  message: string;
}

export interface UpdateProfileRequest {
  first_name?: string | null;
  last_name?: string | null;
  email_notifications?: boolean | null;
  marketing_emails?: boolean | null;
}

export async function uploadUserAvatar(token: string, file: File) {
  try {
    const formData = new FormData();
    formData.append("avatar", file);

    const response = await fetch(`${API_PROXY_BASE}/auth/me/avatar`, {
      method: "POST",
      headers: authUploadHeaders(token),
      body: formData,
    });
    const result = await parseJson<UserProfile>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to upload avatar") };
    return { data: result as UserProfile };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export interface BillingSummary {
  tenant_id: string;
  customer_external_id: string;
  polar_customer_id: string | null;
  polar_subscription_id: string | null;
  polar_product_id: string | null;
  plan_name: string | null;
  status: string;
  amount_cents: number | null;
  currency: string | null;
  current_period_start: string | null;
  current_period_end: string | null;
  cancel_at_period_end: boolean;
  default_checkout_product_id: string | null;
  selected_checkout_product_id: string | null;
  is_test_mode: boolean;
  has_billing_record: boolean;
}

export interface BillingCheckoutRequest {
  product_id?: string;
}

export interface BillingCheckoutProductPreferenceRequest {
  product_id?: string | null;
}

export interface BillingRedirectResponse {
  url: string;
}

export interface BillingProduct {
  id: string;
  name: string;
  description: string | null;
  price_amount_cents: number | null;
  currency: string | null;
  recurring_interval: string | null;
  is_archived: boolean;
  is_default_checkout_product: boolean;
}

export interface BillingProductListResponse {
  products: BillingProduct[];
  default_checkout_product_id: string | null;
  last_synced_at: string | null;
}

export interface HealthResponse {
  status: string;
  version: string;
  services?: Record<string, string>;
}

export interface MeshStatus {
  total_workers: number;
  workers: Array<{
    id: string;
    host_id: string;
    hostname: string;
    advertise_address: string;
    wireguard_pubkey: string | null;
    wireguard_ip: string | null;
    wireguard_port: number | null;
    metrics: unknown | null;
    registered_at: string;
    last_seen_at: string;
  }>;
}

export type WorkspaceEventKind =
  | "app_created"
  | "app_updated"
  | "app_deleted"
  | "deployment_changed"
  | "profile_updated"
  | "github_accounts_changed"
  | "billing_updated"
  | "security_rules_changed"
  | "volume_changed"
  | "snapshot_changed"
  | "database_created"
  | "database_updated"
  | "database_deleted"
  | "refresh";

export interface WorkspaceEvent {
  kind: WorkspaceEventKind;
  user_id?: string | null;
  tenant_id?: string | null;
  app_id?: string | null;
  app_name?: string | null;
  deployment_id?: string | null;
  volume_id?: string | null;
  resource_id?: string | null;
}

export interface WorkspaceNotification {
  id: string;
  user_id: string;
  tenant_id?: string | null;
  kind: string;
  title: string;
  body: string;
  route: string;
  entity_name?: string | null;
  resource_id?: string | null;
  metadata: Record<string, unknown>;
  created_at: string;
  read_at?: string | null;
  is_read: boolean;
}

export interface WorkspaceNotificationListResponse {
  notifications: WorkspaceNotification[];
  unread_count: number;
  has_more: boolean;
  next_offset: number;
}

export type AppScaleState = "active" | "scaled_to_zero";

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
  ipv6_address?: string;
  hypervisor?: string;
  git_commit_hash?: string | null;
  git_commit_message?: string | null;
  git_branch?: string | null;
  scale_state?: AppScaleState;
}

export interface LiveDeploymentStatus extends LiveDeploymentInfo {
  scheduled_at: number;
  started_at: number;
  stopped_at: number;
  error_message: string;
}

export interface LogLine {
  line: string;
  timestamp: number;
  deployment_id?: string;
  scale_state?: AppScaleState;
}

export interface VmMetrics {
  app_id: string;
  job_id?: string;
  deployment_id?: string;
  vm_id: string;
  cpu_usage: number;
  ram_used_bytes: number;
  tx_bytes?: number;
  rx_bytes?: number;
  status: string;
  error_message?: string | null;
  ipv6_address?: string | null;
  scale_state?: AppScaleState;
}

export type VmMetricsResponse = VmMetrics;

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
  github_installation_id?: number;
  github_repo_id?: number;
  github_repo_full_name?: string;
  active_deployment_id: string | null;
  desired_replicas: number;
  min_replicas: number;
  max_replicas: number;
  autoscaling_enabled: boolean;
  cpu_threshold: number;
  mem_threshold: number;
  scale_state: AppScaleState;
  created_at: string;
  updated_at?: string;
}

export interface ScaleAppRequest {
  desired_replicas?: number;
  min_replicas?: number;
  max_replicas?: number;
  autoscaling_enabled?: boolean;
  cpu_threshold?: number;
  mem_threshold?: number;
}

export interface CreateAppRequest {
  name: string;
  git_url: string;
  port?: number;
  github_installation_id?: number;
  github_repo_id?: number;
  github_repo_full_name?: string;
}

export interface UpdateAppRequest {
  port: number;
}

export const DEPLOYMENT_CPU_OPTIONS = [1, 2, 3, 4] as const;
export const DEPLOYMENT_MEMORY_OPTIONS = [
  { label: "512M", value: 512 },
  { label: "1G", value: 1024 },
  { label: "2G", value: 2048 },
  { label: "4G", value: 4096 },
] as const;
export const DEPLOYMENT_HYPERVISOR_OPTIONS = [
  { label: "Default", value: "" },
  { label: "Firecracker", value: "firecracker" },
  { label: "Cloud Hypervisor", value: "cloud-hypervisor" },
] as const;

export interface GithubRepo {
  id: number;
  name: string;
  full_name: string;
  private: boolean;
  html_url: string;
  description: string | null;
  installation_id?: number;
}

export interface DeploymentInfo {
  id: string;
  app_id: string;
  build_id: string | null;
  image_tag: string | null;
  job_id?: string | null;
  ipv6_address: string | null;
  status: string;
  vcpus: number;
  memory_mib: number;
  disk_mib: number;
  port: number;
  env_vars?: Record<string, string>;
  git_commit_hash: string | null;
  git_commit_message: string | null;
  git_branch: string | null;
  trigger_source: string;
  scale_state?: AppScaleState;
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
  hypervisor?: string;
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

export interface SecurityRule {
  id: string;
  app_id: string;
  protocol: string;
  port_start: number;
  port_end: number;
  action: string;
  priority: number;
  created_at: string;
}

export interface CreateSecurityRuleRequest {
  protocol: string;
  port_start: number;
  port_end: number;
  action: string;
}

export interface GithubAccount {
  id: string;
  user_id: string;
  installation_id: number;
  github_username: string;
  created_at: string;
}

export interface Volume {
  id: string;
  user_id: string;
  name: string;
  size_mib: number;
  created_at: string;
  updated_at: string;
}

export interface AppVolume {
  app_id: string;
  volume_id: string;
  mount_point: string;
  access_mode: number;
  created_at: string;
}

export interface AttachedVolume extends AppVolume {
  id: string;
  user_id: string;
  name: string;
  size_mib: number;
  pool_name: string;
  updated_at: string;
}

export interface VolumeAttachmentInfo {
  app_id: string;
  app_name: string;
  mount_point: string;
  access_mode: number;
}

export interface VolumeWithAttachments extends Volume {
  attachments: VolumeAttachmentInfo[];
}

export interface VolumeSnapshot {
  id: string;
  volume_id: string;
  name: string;
  created_at: string;
}

export interface CreateVolumeRequest {
  name: string;
  size_mib: number;
}

export interface AttachVolumeRequest {
  volume_id: string;
  mount_point: string;
  access_mode: number;
}

export interface CreateSnapshotRequest {
  name: string;
}

export interface RestoreSnapshotRequest {
  snapshot_name: string;
}

export interface CloneVolumeRequest {
  name: string;
  snapshot_name: string;
}

const authHeaders = (token: string) => ({
  "Content-Type": "application/json",
  Authorization: `Bearer ${token}`,
});

const authUploadHeaders = (token: string) => ({
  Authorization: `Bearer ${token}`,
});

async function parseJson<T>(response: Response): Promise<T | ApiError> {
  try {
    return await response.json();
  } catch {
    return { error: "Invalid JSON response from server" };
  }
}

function getErrorMessage(result: unknown, fallback: string) {
  if (result !== null && typeof result === "object" && "error" in result && typeof (result as ApiError).error === "string") {
    return (result as ApiError).error;
  }
  return fallback;
}

export async function register(data: RegisterRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    const result = await parseJson<RegisterResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Registration failed") };
    return { data: result as RegisterResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function login(data: LoginRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    const result = await parseJson<LoginResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Login failed") };
    return { data: result as LoginResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function health(): Promise<HealthResponse> {
  const response = await fetch(`${API_PROXY_BASE}/health`);
  return response.json();
}

export async function getUserProfile(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/me`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<UserProfile>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch profile") };
    return { data: result as UserProfile };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getMeshStatus(token: string): Promise<{ data?: MeshStatus; error?: string }> {
  try {
    const response = await fetch(`${API_PROXY_BASE}/networking/mesh`, { headers: authHeaders(token) });
    const result = await parseJson<MeshStatus>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch mesh status") };
    return { data: result as MeshStatus };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listProjects(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/projects`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<ProjectInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch projects") };
    return { data: result as ProjectInfo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createProject(token: string, data: CreateProjectRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/projects`, {
      method: "POST",
      headers: {
        ...authHeaders(token),
        "Content-Type": "application/json",
      },
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<ProjectInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create project") };
    return { data: result as ProjectInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getProject(token: string, tenantId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/projects/${encodeURIComponent(tenantId)}`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<ProjectInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch project") };
    return { data: result as ProjectInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function updateProject(token: string, tenantId: string, data: UpdateProjectRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/projects/${encodeURIComponent(tenantId)}`, {
      method: "PATCH",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<ProjectInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to update project") };
    return { data: result as ProjectInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteProject(token: string, tenantId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/projects/${encodeURIComponent(tenantId)}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    if (response.status === 204) {
      return { data: true };
    }
    const result = await parseJson<ApiError>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to delete project") };
    return { data: true };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listDatabases(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch databases") };
    return { data: result as DatabaseInfo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createDatabase(token: string, data: CreateDatabaseRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create database") };
    return { data: result as DatabaseInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteDatabase(token: string, databaseId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete database") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getDatabaseConnection(token: string, databaseId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/connection`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseConnectionInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch database connection info") };
    return { data: result as DatabaseConnectionInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listDatabaseBranches(token: string, databaseId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/branches`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseBranchInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch database branches") };
    return { data: result as DatabaseBranchInfo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getDatabaseBackupInfo(token: string, databaseId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/backups`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseBackupInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch database backup info") };
    return { data: result as DatabaseBackupInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listDatabaseSnapshots(token: string, databaseId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/backups/snapshots`, {
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseSnapshotListResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch database snapshots") };
    return { data: result as DatabaseSnapshotListResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createDatabaseSnapshot(token: string, databaseId: string, data: { name: string }) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/backups/snapshots`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseSnapshotActionResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create database snapshot") };
    return { data: result as DatabaseSnapshotActionResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function restoreDatabaseSnapshot(token: string, databaseId: string, data: { snapshot_name: string }) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/backups/restore`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseSnapshotActionResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to restore database snapshot") };
    return { data: result as DatabaseSnapshotActionResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteDatabaseSnapshot(token: string, databaseId: string, snapshotName: string) {
  try {
    const response = await fetch(
      `${API_PROXY_BASE}/databases/${encodeURIComponent(databaseId)}/backups/snapshots/${encodeURIComponent(snapshotName)}`,
      {
        method: "DELETE",
        headers: authHeaders(token),
      },
    );
    if (response.status === 401) logout();
    const result = await parseJson<DatabaseSnapshotActionResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to delete database snapshot") };
    return { data: result as DatabaseSnapshotActionResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function updateUserProfile(token: string, data: UpdateProfileRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/me`, {
      method: "PUT",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<UserProfile>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to update profile") };
    return { data: result as UserProfile };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getBillingSummary(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/billing`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<BillingSummary>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch billing summary") };
    return { data: result as BillingSummary };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listBillingProducts(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/billing/products`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<BillingProductListResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch billing products") };
    return { data: result as BillingProductListResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function refreshBillingProducts(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/billing/products/refresh`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<BillingProductListResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to refresh billing products") };
    return { data: result as BillingProductListResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function updateBillingCheckoutProduct(
  token: string,
  data: BillingCheckoutProductPreferenceRequest,
) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/billing/checkout-product`, {
      method: "PUT",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<BillingSummary>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to update checkout product") };
    return { data: result as BillingSummary };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createBillingCheckout(token: string, data: BillingCheckoutRequest = {}) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/billing/checkout`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    const result = await parseJson<BillingRedirectResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create billing checkout") };
    return { data: result as BillingRedirectResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createBillingPortal(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/billing/portal`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    const result = await parseJson<BillingRedirectResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create billing portal link") };
    return { data: result as BillingRedirectResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listActiveDeployments(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/deployments/active`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<LiveDeploymentInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch active deployments") };
    return { data: result as LiveDeploymentInfo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getLiveDeploymentStatus(token: string, appName: string, jobId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/deployments/${jobId}`, { headers: authHeaders(token) });
    const result = await parseJson<LiveDeploymentStatus>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch deployment status") };
    return { data: result as LiveDeploymentStatus };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

function eventSourceStream(url: string, onMessage: (payload: unknown) => void) {
  const source = new EventSource(url);
  source.onmessage = (event) => {
    try {
      onMessage(JSON.parse(event.data));
    } catch {
      // Ignore malformed events.
    }
  };
  source.onerror = () => {};
  return () => source.close();
}

export function watchDeploymentsSSE(token: string, onMessage: (deployment: LiveDeploymentInfo) => void) {
  return createFetchSseStream(
    `${API_PROXY_BASE}/deployments/events`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
    (payload) => onMessage(payload as LiveDeploymentInfo),
  );
}

export function watchAppMetricsSSE(token: string, appName: string, onMessage: (metrics: VmMetricsResponse) => void) {
  return createFetchSseStream(
    `${API_PROXY_BASE}/apps/${encodeURIComponent(appName)}/metrics/stream`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
    (payload) => onMessage(payload as VmMetricsResponse),
  );
}

export function watchMeshStatusSSE(token: string, onMessage: (mesh: MeshStatus) => void) {
  return createFetchSseStream(
    `${API_PROXY_BASE}/networking/mesh/stream`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
    (payload) => onMessage(payload as MeshStatus),
  );
}

export function watchWorkspaceEventsSSE(token: string, onMessage: (event: WorkspaceEvent) => void) {
  return createFetchSseStream(
    `${API_PROXY_BASE}/workspace/events`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
    (payload) => onMessage(payload as WorkspaceEvent),
  );
}

export async function listNotifications(
  token: string,
  options: { limit?: number; offset?: number; unreadOnly?: boolean } = {},
) {
  try {
    const params = new URLSearchParams();
    if (typeof options.limit === "number") params.set("limit", String(options.limit));
    if (typeof options.offset === "number") params.set("offset", String(options.offset));
    if (typeof options.unreadOnly === "boolean") params.set("unread_only", String(options.unreadOnly));

    const response = await fetch(
      `${API_PROXY_BASE}/notifications${params.toString() ? `?${params.toString()}` : ""}`,
      {
        headers: authHeaders(token),
      },
    );
    const result = await parseJson<WorkspaceNotificationListResponse>(response);
    if (!response.ok) {
      return { error: getErrorMessage(result, "Failed to fetch notifications") };
    }
    return { data: result as WorkspaceNotificationListResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function markNotificationRead(token: string, notificationId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/notifications/${notificationId}/read`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to mark notification as read") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function markAllNotificationsRead(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/notifications/read-all`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to mark notifications as read") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export function watchHealthSSE(onMessage: (health: HealthResponse) => void) {
  return eventSourceStream(`${API_PROXY_BASE}/health/stream`, (payload) =>
    onMessage(payload as HealthResponse)
  );
}

export function watchAppLogsSSE(token: string, appName: string, onMessage: (logs: LogLine | LogLine[]) => void) {
  return createFetchSseStream(
    `${API_PROXY_BASE}/apps/${encodeURIComponent(appName)}/logs/stream`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
    (payload) => onMessage(payload as LogLine | LogLine[]),
  );
}

export async function listSecurityRules(token: string, appName: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/security-groups`, { headers: authHeaders(token) });
    const result = await parseJson<SecurityRule[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch security rules") };
    return { data: result as SecurityRule[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createSecurityRule(token: string, appName: string, data: CreateSecurityRuleRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/security-groups`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<SecurityRule>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create security rule") };
    return { data: result as SecurityRule };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteSecurityRule(token: string, appName: string, ruleId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/security-groups/${ruleId}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete security rule") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listApps(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<AppInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch apps") };
    return { data: result as AppInfo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getAppSecret(token: string, appName: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/secret`, { headers: authHeaders(token) });
    const result = await parseJson<{ github_webhook_secret: string | null }>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch app secret") };
    return { data: result as { github_webhook_secret: string | null } };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createApp(token: string, data: CreateAppRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<AppInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create app") };
    return { data: result as AppInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function updateApp(token: string, appName: string, data: UpdateAppRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${encodeURIComponent(appName)}`, {
      method: "PATCH",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<AppInfo>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to update app") };
    return { data: result as AppInfo };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function scaleApp(token: string, appName: string, data: ScaleAppRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/scale`, {
      method: "PATCH",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to scale app") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteApp(token: string, appName: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete app") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deployApp(token: string, data: DeployRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/deploy`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<DeployResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to start deployment") };
    return { data: result as DeployResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deployAppVersion(token: string, appName: string, data: Partial<DeployRequest> = {}) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/deploy`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<DeployResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to start deployment") };
    return { data: result as DeployResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listDeployments(token: string, appName: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/deployments`, { headers: authHeaders(token) });
    const result = await parseJson<DeploymentInfo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch deployments") };
    return { data: result as DeploymentInfo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function activateDeployment(token: string, appName: string, deploymentId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appName}/deployments/${deploymentId}/activate`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to activate deployment") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listGithubAccounts(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/github/accounts`, { headers: authHeaders(token) });
    const result = await parseJson<GithubAccount[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch GitHub accounts") };
    return { data: result as GithubAccount[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listGithubRepos(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/github/repos`, { headers: authHeaders(token) });
    const result = await parseJson<GithubRepo[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch GitHub repositories") };
    return { data: result as GithubRepo[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function getGithubInstallUrl(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/github/install`, { headers: authHeaders(token) });
    const result = await parseJson<{ url: string }>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to get GitHub installation URL") };
    return { data: result as { url: string } };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listVolumes(token: string, appId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appId}/volumes`, { headers: authHeaders(token) });
    const result = await parseJson<AttachedVolume[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch volumes") };
    return { data: result as AttachedVolume[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listAllVolumes(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes`, { headers: authHeaders(token) });
    const result = await parseJson<VolumeWithAttachments[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch all volumes") };
    return { data: result as VolumeWithAttachments[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createVolume(token: string, data: CreateVolumeRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<Volume>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create volume") };
    return { data: result as Volume };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function attachVolume(token: string, appId: string, data: AttachVolumeRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appId}/volumes/attach`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<AppVolume>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to attach volume") };
    return { data: result as AppVolume };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function detachVolume(token: string, appId: string, volumeId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/apps/${appId}/volumes/${volumeId}/detach`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (!response.ok) {
      const result = await parseJson(response);
      return { error: getErrorMessage(result, "Failed to detach volume") };
    }
    return { data: true };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createVolumeSnapshot(token: string, volumeId: string, data: CreateSnapshotRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes/${volumeId}/snapshots`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<VolumeSnapshot>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create snapshot") };
    return { data: result as VolumeSnapshot };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listVolumeSnapshots(token: string, volumeId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes/${volumeId}/snapshots`, { headers: authHeaders(token) });
    const result = await parseJson<VolumeSnapshot[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to fetch snapshots") };
    return { data: result as VolumeSnapshot[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function restoreVolumeSnapshot(token: string, volumeId: string, data: RestoreSnapshotRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes/${volumeId}/restore`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to restore snapshot") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function cloneVolumeFromSnapshot(token: string, volumeId: string, data: CloneVolumeRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes/${volumeId}/clone`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(data),
    });
    const result = await parseJson<Volume>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to clone volume") };
    return { data: result as Volume };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteVolumeSnapshot(token: string, snapshotId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/snapshots/${snapshotId}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete snapshot") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteVolume(token: string, volumeId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/volumes/${volumeId}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete volume") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function listVms(token: string) {
  return listActiveDeployments(token);
}

export const watchVmsSSE = watchDeploymentsSSE;
export const watchAppMetrics = watchAppMetricsSSE;
export const watchAppLogs = watchAppLogsSSE;
export const watchMeshStatus = watchMeshStatusSSE;
export const watchWorkspaceEvents = watchWorkspaceEventsSSE;
export const getNotifications = listNotifications;
export const readNotification = markNotificationRead;
export const readAllNotifications = markAllNotificationsRead;
export const getVmStatus = getLiveDeploymentStatus;
export const getVm = getVmStatus;
export const getVmLogsSSE = watchAppLogsSSE;
export const pauseVm = async (_token: string, _appName: string, _jobId: string) => ({ success: false, error: "Not implemented" });
export const resumeVm = async (_token: string, _appName: string, _jobId: string) => ({ success: false, error: "Not implemented" });
export const stopVm = async (_token: string, _appName: string, _jobId: string) => ({ success: false, error: "Not implemented" });
export const deleteVm = async (_token: string, _appName: string, _jobId: string) => ({ success: false, error: "Not implemented" });

export interface ChangePasswordRequest {
  current_password: string;
  new_password: string;
}

export async function changePassword(token: string, data: ChangePasswordRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/password`, {
      method: "POST",
      headers: { ...authHeaders(token), "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to change password") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export interface TotpSetupResponse {
  secret: string;
  otpauth_url: string;
}

export async function setupTotp(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/2fa/setup`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<TotpSetupResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to setup 2FA") };
    return { data: result as TotpSetupResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export interface VerifyTotpRequest {
  code: string;
}

export async function verifyTotp(token: string, data: VerifyTotpRequest) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/2fa/verify`, {
      method: "POST",
      headers: { ...authHeaders(token), "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    if (response.status === 401) logout();
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to verify 2FA code") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function disableTotp(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/2fa/disable`, {
      method: "POST",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to disable 2FA") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function deleteAccount(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/me`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to delete account") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}

export interface PersonalAccessToken {
  id: string;
  user_id: string;
  name: string;
  token_last_four: string;
  created_at: string;
  last_used_at: string | null;
}

export interface CreatedTokenResponse {
  token: string;
  details: PersonalAccessToken;
}

export async function listPersonalAccessTokens(token: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/tokens`, { headers: authHeaders(token) });
    if (response.status === 401) logout();
    const result = await parseJson<PersonalAccessToken[]>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to load tokens") };
    return { data: result as PersonalAccessToken[] };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function createPersonalAccessToken(token: string, name: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/tokens`, {
      method: "POST",
      headers: { ...authHeaders(token), "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    if (response.status === 401) logout();
    const result = await parseJson<CreatedTokenResponse>(response);
    if (!response.ok) return { error: getErrorMessage(result, "Failed to create token") };
    return { data: result as CreatedTokenResponse };
  } catch (error) {
    return { error: error instanceof Error ? error.message : "Network error" };
  }
}

export async function revokePersonalAccessToken(token: string, tokenId: string) {
  try {
    const response = await fetch(`${API_PROXY_BASE}/auth/tokens/${tokenId}`, {
      method: "DELETE",
      headers: authHeaders(token),
    });
    if (response.status === 401) logout();
    if (response.ok) return { success: true };
    const result = await parseJson<ApiError>(response);
    return { success: false, error: getErrorMessage(result, "Failed to revoke token") };
  } catch (error) {
    return { success: false, error: error instanceof Error ? error.message : "Network error" };
  }
}
