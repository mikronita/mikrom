const authPayload = Buffer.from(
  JSON.stringify({
    exp: Math.floor(Date.now() / 1000) + 60 * 60,
  })
).toString("base64");

export const authToken = `header.${authPayload}.signature`;

export const profile = {
  id: "user-1",
  email: "admin@mikrom.io",
  role: "admin",
  first_name: "Ada",
  last_name: "Lovelace",
  vpc_ipv6_prefix: "fd00:1234::/40",
};

export const projects = [
  {
    id: "project-1",
    tenant_id: "acme",
    name: "Acme",
    created_at: "2026-05-01T10:00:00.000Z",
    updated_at: "2026-05-01T10:00:00.000Z",
  },
];

export const apps = [
  {
    id: "app-1",
    name: "starter",
    git_url: "https://github.com/mikrom/starter",
    port: 3000,
    hostname: null,
    active_deployment_id: null,
    desired_replicas: 1,
    min_replicas: 1,
    max_replicas: 1,
    autoscaling_enabled: false,
    cpu_threshold: 80,
    mem_threshold: 80,
    scale_state: "active",
    created_at: "2026-05-02T10:00:00.000Z",
    updated_at: "2026-05-04T10:00:00.000Z",
  },
];

export const appDeployments = [
  {
    id: "deploy-1",
    app_id: "app-1",
    build_id: "build-1",
    image_tag: "ghcr.io/mikrom/starter:1.0.0",
    job_id: "job-1",
    ipv6_address: "fd00:1234::10",
    status: "RUNNING",
    vcpus: 1,
    memory_mib: 512,
    disk_mib: 1024,
    port: 3000,
    env_vars: {},
    git_commit_hash: "abcdef1234567890",
    git_commit_message: "Initial stable release",
    git_branch: "main",
    trigger_source: "github_webhook",
    created_at: "2026-05-02T10:30:00.000Z",
    updated_at: "2026-05-02T10:30:00.000Z",
  },
  {
    id: "deploy-2",
    app_id: "app-1",
    build_id: "build-2",
    image_tag: "ghcr.io/mikrom/starter:1.1.0",
    job_id: "job-2",
    ipv6_address: "fd00:1234::11",
    status: "STOPPED",
    vcpus: 1,
    memory_mib: 512,
    disk_mib: 1024,
    port: 3000,
    env_vars: {},
    git_commit_hash: "fedcba0987654321",
    git_commit_message: "Preview candidate",
    git_branch: "feature/preview",
    trigger_source: "manual",
    created_at: "2026-05-03T10:30:00.000Z",
    updated_at: "2026-05-03T10:30:00.000Z",
  },
];

export const deployments = [
  {
    job_id: "job-1",
    deployment_id: "deploy-1",
    app_id: "app-1",
    app_name: "starter",
    image: "ghcr.io/mikrom/starter:latest",
    status: "RUNNING",
    host_id: "host-1",
    vm_id: "vm-1",
    vcpus: 1,
    memory_mib: 256,
    cpu_usage: 10,
    ram_used_bytes: 131072,
    ipv6_address: "fd00:1234::10",
  },
];

export const mesh = {
  total_workers: 1,
  workers: [
    {
      id: "worker-1",
      host_id: "host-1",
      hostname: "worker-01",
      advertise_address: "10.0.0.1",
      wireguard_pubkey: "wg-key",
      wireguard_ip: "fd00:1234::1",
      wireguard_port: 51823,
      metrics: null,
      registered_at: "2026-05-01T10:00:00.000Z",
      last_seen_at: "2026-05-01T10:05:00.000Z",
    },
  ],
};

export const securityRules = [
  {
    id: "rule-1",
    app_id: "app-1",
    protocol: "tcp",
    port_start: 80,
    port_end: 80,
    action: "allow",
    priority: 100,
    created_at: "2026-05-02T10:00:00.000Z",
  },
];

export const volumes = [
  {
    id: "vol-1",
    user_id: "user-1",
    name: "app-data",
    size_mib: 1024,
    created_at: "2026-05-01T10:00:00.000Z",
    updated_at: "2026-05-01T10:00:00.000Z",
    attachments: [
      {
        app_id: "app-1",
        app_name: "starter",
        mount_point: "/data",
        access_mode: 0,
      },
    ],
  },
];

export const volumeSnapshots = [
  {
    id: "snap-1",
    volume_id: "vol-1",
    name: "daily-backup",
    created_at: "2026-05-03T10:00:00.000Z",
  },
];
