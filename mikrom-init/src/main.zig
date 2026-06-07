const std = @import("std");
const ascii = std.ascii;
const mem = std.mem;
const net = std.Io.net;
const linux = std.os.linux;

const CONFIG_PATH = "/etc/mikrom/init.json";
const FALLBACK_SHELL = "/bin/sh";
const DEFAULT_NEON_PAGESERVER_IPV6 = "fd00::deed:1d1c";
const DATABASE_ID_ENV = "MIKROM_DATABASE_ID";
const NEON_JWKS_JSON_ENV = "NEON_JWKS_JSON";
const NEON_JWKS_PATH_ENV = "NEON_JWKS_PATH";
const NEON_INSTANCE_ID_ENV = "NEON_INSTANCE_ID";
const NEON_SAFEKEEPERS_GENERATION_ENV = "NEON_SAFEKEEPERS_GENERATION";
const NEON_SAFEKEEPER_CONNSTRS_ENV = "NEON_SAFEKEEPER_CONNSTRS";
const NEON_DEV_MODE_ENV = "MIKROM_NEON_DEV_MODE";
const TRACE_FILE_OPS_ENV = "MIKROM_INIT_TRACE_FILES";
const STRACE_BINARIES = [_][]const u8{ "/usr/bin/strace", "/bin/strace" };

pub const VolumeConfig = struct {
    drive_id: []const u8,
    mount_point: []const u8,
    index: ?usize = null,
};

pub const WorkloadType = enum {
    APP,
    DATABASE,
};

pub const InitConfig = struct {
    env: std.json.Value = .{ .object = .empty },
    workdir: []const u8 = "/app",
    entrypoint: [][]const u8 = &.{},
    cmd: [][]const u8 = &.{},
    volumes: []VolumeConfig = &.{},
    workload_type: WorkloadType = .APP,
};

// Netlink constants and structs
const AF_NETLINK = 16;
const NETLINK_ROUTE = 0;

const RTM_NEWLINK = 16;
const RTM_GETLINK = 18;
const RTM_NEWADDR = 20;
const RTM_NEWROUTE = 24;

const NLM_F_REQUEST = 1;
const NLM_F_ACK = 4;
const NLM_F_EXCL = 0x200;
const NLM_F_CREATE = 0x400;
const NLM_F_DUMP = 0x100;

const IFF_UP = 0x1;

const RTA_GATEWAY = 5;
const RTA_OIF = 4;
const RTA_DST = 1;

const IFA_ADDRESS = 1;
const IFA_LOCAL = 2;

const IFLA_IFNAME = 3;
const IFLA_MTU = 4;

const nlmsghdr = extern struct {
    len: u32,
    type: u16,
    flags: u16,
    seq: u32,
    pid: u32,
};

const ifinfomsg = extern struct {
    family: u8,
    pad: u8 = 0,
    type: u16 = 0,
    index: i32,
    flags: u32,
    change: u32,
};

const ifaddrmsg = extern struct {
    family: u8,
    prefixlen: u8,
    flags: u8,
    scope: u8,
    index: u32,
};

const rtmsg = extern struct {
    family: u8,
    dst_len: u8,
    src_len: u8,
    tos: u8,
    table: u8,
    protocol: u8,
    scope: u8,
    type: u8,
    flags: u32,
};

const rtattr = extern struct {
    len: u16,
    type: u16,
};

pub fn main(init: std.process.Init) !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    std.debug.print("[mikrom-init] Initializing microVM environment...\n", .{});

    setup_mounts(allocator) catch |err| {
        std.debug.print("[mikrom-init] Warning: Mount setup encountered errors: {any}\n", .{err});
    };

    setup_system(allocator) catch |err| {
        std.debug.print("[mikrom-init] Warning: System setup encountered errors: {any}\n", .{err});
    };

    const config = load_config(init.io, allocator, CONFIG_PATH) catch |err| {
        std.debug.print("[mikrom-init] Error loading configuration: {any}\n", .{err});
        fallback_to_shell();
    };

    setup_networking(init.io, allocator, config) catch |err| {
        std.debug.print("[mikrom-init] Warning: Networking setup encountered errors: {any}\n", .{err});
    };

    setup_volume_mounts(init.io, allocator, config) catch |err| {
        std.debug.print("[mikrom-init] Warning: Volume mounting encountered errors: {any}\n", .{err});
    };

    start_background_services(init.io, allocator) catch |err| {
        std.debug.print("[mikrom-init] Warning: Failed to start background services: {any}\n", .{err});
    };

    switch (config.workload_type) {
        .APP => run_app(init.io, allocator, init.environ_map, config),
        .DATABASE => run_database(init.io, allocator, init.environ_map, config),
    }
}

fn run_app(io: std.Io, allocator: mem.Allocator, base_env: *const std.process.Environ.Map, config: InitConfig) void {
    if (config.entrypoint.len == 0 and config.cmd.len == 0) {
        std.debug.print("[mikrom-init] Error: No entrypoint or cmd provided for app\n", .{});
        fallback_to_shell();
    }

    std.debug.print("[mikrom-init] Starting application: {any}\n", .{config.entrypoint});

    const port_str = envGet(config.env, "PORT") orelse "8080";
    const port = std.fmt.parseInt(u16, port_str, 10) catch 8080;

    var child = spawn_application(io, allocator, base_env, config) catch |err| {
        std.debug.print("[mikrom-init] Warning: Failed to spawn application: {any}\n", .{err});
        fallback_to_shell();
    };

    wait_for_port_ready(io, port, &child) catch |err| {
        std.debug.print("[mikrom-init] Application never became ready: {any}\n", .{err});
        fallback_to_shell();
    };

    std.debug.print("__MIKROM_APP_START__\n", .{});

    const term = child.wait(io) catch |err| {
        std.debug.print("[mikrom-init] Failed while waiting for application exit: {any}\n", .{err});
        fallback_to_shell();
    };
    std.debug.print("[mikrom-init] Application exited with {any}\n", .{term});

    fallback_to_shell();
}

fn run_database(io: std.Io, allocator: mem.Allocator, base_env: *const std.process.Environ.Map, config: InitConfig) void {
    std.debug.print("[mikrom-init] Starting database (Neon Compute Node)...\n", .{});

    run_database_impl(io, allocator, base_env, config) catch |err| {
        std.debug.print("[mikrom-init] Database error: {any}\n", .{err});
        fallback_to_shell();
    };
}

fn run_database_impl(io: std.Io, allocator: mem.Allocator, base_env: *const std.process.Environ.Map, config: InitConfig) !void {
    std.debug.print("[mikrom-init] Preparing Neon Compute Node...\n", .{});

    var tmp_dir = try std.Io.Dir.openDirAbsolute(io, "/tmp", .{});
    defer tmp_dir.close(io);
    tmp_dir.deleteTree(io, "pgdata") catch |err| {
        std.debug.print("[mikrom-init] Warning: Failed to clean /tmp/pgdata: {any}\n", .{err});
    };

    var child = try spawn_database(io, allocator, base_env, config);
    dump_pgdata_state(io, "/tmp");

    std.debug.print("[mikrom-init] Launching Postgres...\n", .{});

    wait_for_port_ready(io, 5432, &child) catch |err| {
        dump_pgdata_state(io, "/tmp/pgdata");
        return err;
    };
    std.debug.print("__MIKROM_DB_START__\n", .{});

    const term = child.wait(io) catch |err| {
        dump_pgdata_state(io, "/tmp/pgdata");
        std.debug.print("[mikrom-init] Error supervising Postgres: {any}\n", .{err});
        return err;
    };
    std.debug.print("[mikrom-init] Postgres exited with {any}. Environment may need restart.\n", .{term});
}

fn setup_mounts(allocator: mem.Allocator) !void {
    try mount_fs(allocator, "proc", "/proc", "proc", 0);
    try mount_fs(allocator, "sysfs", "/sys", "sysfs", 0);

    mount_fs(allocator, "devtmpfs", "/dev", "devtmpfs", 0) catch |err| {
        std.debug.print("[mikrom-init] Warning: Failed to mount /dev: {any}\n", .{err});
    };

    const tmp_dirs = [_][]const u8{ "/run", "/tmp", "/dev/pts", "/dev/shm" };
    for (tmp_dirs) |dir| {
        makePath(allocator, dir) catch {};
    }

    try mount_fs(allocator, "tmpfs", "/run", "tmpfs", 0);
    try mount_fs(allocator, "tmpfs", "/tmp", "tmpfs", 0);
    try mount_fs(allocator, "tmpfs", "/dev/shm", "tmpfs", 0);

    mount_fs(allocator, "devpts", "/dev/pts", "devpts", 0) catch |err| {
        std.debug.print("[mikrom-init] Warning: Failed to mount /dev/pts: {any}\n", .{err});
    };
}

fn mount_fs(allocator: mem.Allocator, source: []const u8, target: []const u8, fstype: []const u8, flags: usize) !void {
    try makePath(allocator, target);
    const source_z = try dupeZ(allocator, source);
    const target_z = try dupeZ(allocator, target);
    const fstype_z = try dupeZ(allocator, fstype);
    
    const res = linux.mount(source_z, target_z, fstype_z, @as(u32, @intCast(flags)), 0);
    if (res != 0) return error.MountFailed;
}

fn makePath(allocator: mem.Allocator, path: []const u8) !void {
    var i: usize = 1;
    while (i <= path.len) : (i += 1) {
        if (i == path.len or path[i] == '/') {
            const sub = path[0..i];
            const sub_z = try dupeZ(allocator, sub);
            defer allocator.free(sub_z);
            _ = linux.mkdirat(linux.AT.FDCWD, sub_z, 0o755);
        }
    }
}

fn dupeZ(allocator: mem.Allocator, s: []const u8) ![:0]u8 {
    const buf = try allocator.alloc(u8, s.len + 1);
    @memcpy(buf[0..s.len], s);
    buf[s.len] = 0;
    return buf[0..s.len :0];
}

fn dupe(allocator: mem.Allocator, s: []const u8) ![]u8 {
    const buf = try allocator.alloc(u8, s.len);
    @memcpy(buf, s);
    return buf;
}

const RunAsUser = struct {
    uid: u32,
    gid: u32,
    dir: []const u8,
    name: []const u8,
};

const DatabaseClusterConfig = struct {
    cluster_id: []const u8,
    tenant_id: []const u8,
    timeline_id: []const u8,
    mode: []const u8 = "Primary",
};

const DatabaseComputeCtlConfig = struct {
    cluster: DatabaseClusterConfig,
    pageserver_connstr: []const u8,
    safekeeper_connstrs: [][]const u8,
    jwks: std.json.Value,
    safekeepers_generation: u32 = 1,
    instance_id: ?[]const u8 = null,

    fn jsonStringify(self: @This(), jws: anytype) !void {
        try jws.beginObject();
        try jws.objectField("cluster");
        try jws.write(self.cluster);
        try jws.objectField("pageserver_connstr");
        try jws.write(self.pageserver_connstr);
        try jws.objectField("safekeeper_connstrs");
        try jws.write(self.safekeeper_connstrs);
        try jws.objectField("jwks");
        try jws.write(self.jwks);
        try jws.objectField("safekeepers_generation");
        try jws.write(self.safekeepers_generation);
        if (self.instance_id) |instance_id| {
            try jws.objectField("instance_id");
            try jws.write(instance_id);
        }
        try jws.endObject();
    }
};

const DatabaseConfig = struct {
    compute_ctl_config: DatabaseComputeCtlConfig,
};

fn spawn_database(io: std.Io, allocator: mem.Allocator, base_env: *const std.process.Environ.Map, config: InitConfig) !std.process.Child {
    const user = try resolve_mikrom_user(io, allocator);
    const tenant_id = envGet(config.env, "NEON_TENANT_ID") orelse return error.MissingTenantId;
    const timeline_id = envGet(config.env, "NEON_TIMELINE_ID") orelse return error.MissingTimelineId;
    const pageserver_ipv6 = envGet(config.env, "NEON_PAGESERVER_IPV6") orelse DEFAULT_NEON_PAGESERVER_IPV6;
    const pageserver_host = try neon_host_alias(allocator, "neon-pageserver", pageserver_ipv6);
    defer allocator.free(pageserver_host);
    try ensure_etc_hosts_entry(io, allocator, pageserver_host, pageserver_ipv6);

    const safekeeper_connstrs = try normalize_neon_safekeeper_connstrings(
        io,
        allocator,
        envGet(config.env, NEON_SAFEKEEPER_CONNSTRS_ENV),
        "neon-safekeeper",
        pageserver_host,
    );
    const compute_id = envGet(config.env, DATABASE_ID_ENV) orelse return error.MissingDatabaseId;

    std.debug.print("[mikrom-init] Tenant: {s}, Timeline: {s}\n", .{ tenant_id, timeline_id });

    const trace_file_ops = if (envGet(config.env, TRACE_FILE_OPS_ENV)) |flag|
        try parse_bool_flag(flag)
    else
        false;

    const program = if (trace_file_ops) blk: {
        if (find_existing_binary(STRACE_BINARIES[0..])) |strace_bin| break :blk strace_bin;
        std.debug.print(
            "[mikrom-init] Warning: MIKROM_INIT_TRACE_FILES is set, but strace is not installed; launching compute_ctl directly\n",
            .{},
        );
        break :blk "/usr/local/bin/compute_ctl";
    } else "/usr/local/bin/compute_ctl";

    const config_file_path = "/tmp/compute_config.json";
    const config_json = try build_database_config_json(io, allocator, config, compute_id, tenant_id, timeline_id, pageserver_host, safekeeper_connstrs);
    defer allocator.free(config_json);
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = config_file_path, .data = config_json });

    const dummy_connstr = try std.fmt.allocPrint(allocator, "postgresql://cloud_admin@localhost:6400/{s}?options=-c%20neon.timeline_id={s}", .{ tenant_id, timeline_id });
    defer allocator.free(dummy_connstr);

    const effective = try effective_path(allocator, envGet(config.env, "PATH"));
    defer allocator.free(effective);

    var env_map = try base_env.clone(allocator);
    defer env_map.deinit();

    if (config.env == .object) {
        var it = config.env.object.iterator();
        while (it.next()) |entry| {
            if (entry.value_ptr.* != .string) continue;
            if (mem.eql(u8, entry.key_ptr.*, "LD_LIBRARY_PATH")) continue;
            try env_map.put(entry.key_ptr.*, entry.value_ptr.string);
        }
    }

    try env_map.put("LD_LIBRARY_PATH", "/usr/local/postgresql/lib:/lib:/usr/lib/x86_64-linux-gnu");
    try env_map.put("PATH", effective);
    try env_map.put("HOME", user.dir);
    try env_map.put("USER", user.name);
    try env_map.put("LOGNAME", user.name);
    try env_map.put("NEON_PAGESERVER_CONNSTR", try std.fmt.allocPrint(allocator, "host={s} port=6400", .{pageserver_host}));
    try env_map.put("NEON_SAFEKEEPERS_GENERATION", try std.fmt.allocPrint(allocator, "{d}", .{compute_safekeeper_generation(config.env)}));
    try env_map.put("NEON_SAFEKEEPER_CONNSTRS", try join_strings(allocator, safekeeper_connstrs, ","));

    const dev_mode = if (envGet(config.env, NEON_DEV_MODE_ENV)) |flag|
        try parse_bool_flag(flag)
    else
        true;

    var argv = std.array_list.Managed([]const u8).init(allocator);
    defer argv.deinit();

    if (trace_file_ops and find_existing_binary(STRACE_BINARIES[0..]) != null) {
        try argv.appendSlice(&.{
            program,
            "-f",
            "-e",
            "trace=file",
            "-s",
            "256",
            "-o",
            "/tmp/compute_ctl.strace",
            "/usr/local/bin/compute_ctl",
        });
    } else {
        try argv.append(program);
    }

    try argv.appendSlice(&.{
        "--pgbin",
        "/usr/local/postgresql/bin/postgres",
        "--pgdata",
        "/tmp/pgdata",
        "--compute-id",
        compute_id,
        "--connstr",
        dummy_connstr,
        "--config",
        config_file_path,
    });

    if (dev_mode) {
        try argv.append("--dev");
    }

    return try std.process.spawn(io, .{
        .argv = argv.items,
        .cwd = .{ .path = config.workdir },
        .environ_map = &env_map,
        .uid = user.uid,
        .gid = user.gid,
    });
}

fn build_database_config_json(
    io: std.Io,
    allocator: mem.Allocator,
    config: InitConfig,
    compute_id: []const u8,
    tenant_id: []const u8,
    timeline_id: []const u8,
    pageserver_host: []const u8,
    safekeeper_connstrs: [][]const u8,
) ![]u8 {
    const raw_jwks = try resolve_neon_jwks(io, allocator, config.env);
    const safekeeper_generation_value = compute_safekeeper_generation(config.env);
    const instance_id = envGet(config.env, NEON_INSTANCE_ID_ENV);
    const data = DatabaseConfig{
        .compute_ctl_config = .{
            .cluster = .{
                .cluster_id = compute_id,
                .tenant_id = tenant_id,
                .timeline_id = timeline_id,
            },
            .pageserver_connstr = try std.fmt.allocPrint(allocator, "host={s} port=6400", .{pageserver_host}),
            .safekeeper_connstrs = safekeeper_connstrs,
            .jwks = raw_jwks,
            .safekeepers_generation = safekeeper_generation_value,
            .instance_id = if (instance_id) |id| blk: {
                if (std.mem.trim(u8, id, " \t\r\n").len == 0) break :blk null;
                break :blk id;
            } else null,
        },
    };
    return try std.json.Stringify.valueAlloc(allocator, data, .{});
}

fn compute_safekeeper_generation(env: std.json.Value) u32 {
    return if (envGet(env, NEON_SAFEKEEPERS_GENERATION_ENV)) |raw|
        std.fmt.parseInt(u32, std.mem.trim(u8, raw, " \t\r\n"), 10) catch 1
    else
        1;
}

fn find_existing_binary(paths: []const []const u8) ?[]const u8 {
    for (paths) |path| {
        if (pathExists(path)) return path;
    }
    return null;
}

fn join_strings(allocator: mem.Allocator, items: [][]const u8, sep: []const u8) ![]u8 {
    if (items.len == 0) return try allocator.dupe(u8, "");
    var total: usize = 0;
    for (items) |item| total += item.len;
    total += sep.len * (items.len - 1);
    const out = try allocator.alloc(u8, total);
    var i: usize = 0;
    for (items, 0..) |item, idx| {
        if (idx > 0) {
            @memcpy(out[i .. i + sep.len], sep);
            i += sep.len;
        }
        @memcpy(out[i .. i + item.len], item);
        i += item.len;
    }
    return out;
}

fn parse_bool_flag(value: []const u8) !bool {
    const trimmed = std.mem.trim(u8, value, " \t\r\n");
    if (ascii.eqlIgnoreCase(trimmed, "1") or
        ascii.eqlIgnoreCase(trimmed, "true") or
        ascii.eqlIgnoreCase(trimmed, "yes") or
        ascii.eqlIgnoreCase(trimmed, "on"))
        return true;
    if (ascii.eqlIgnoreCase(trimmed, "0") or
        ascii.eqlIgnoreCase(trimmed, "false") or
        ascii.eqlIgnoreCase(trimmed, "no") or
        ascii.eqlIgnoreCase(trimmed, "off"))
        return false;
    return error.InvalidBoolean;
}

fn neon_host_alias(allocator: mem.Allocator, prefix: []const u8, value: []const u8) ![]u8 {
    var out = std.array_list.Managed(u8).init(allocator);
    errdefer out.deinit();
    try out.appendSlice(prefix);
    try out.append('-');
    for (value) |ch| {
        if (ascii.isAlphanumeric(ch)) {
            try out.append(ascii.toLower(ch));
        } else {
            try out.append('-');
        }
    }
    return try out.toOwnedSlice();
}

fn resolve_neon_jwks(io: std.Io, allocator: mem.Allocator, env: std.json.Value) !std.json.Value {
    const raw_jwks = if (envGet(env, NEON_JWKS_PATH_ENV)) |path|
        try readFileAlloc(io, allocator, path)
    else if (envGet(env, NEON_JWKS_JSON_ENV)) |raw|
        try allocator.dupe(u8, raw)
    else
        return .{ .object = .empty };

    defer allocator.free(raw_jwks);

    const parsed = try std.json.parseFromSliceLeaky(std.json.Value, allocator, raw_jwks, .{ .allocate = .alloc_always });
    return switch (parsed) {
        .object => parsed,
        .array => blk: {
            const wrapped = try std.fmt.allocPrint(allocator, "{{\"keys\":{s}}}", .{raw_jwks});
            defer allocator.free(wrapped);
            break :blk try std.json.parseFromSliceLeaky(std.json.Value, allocator, wrapped, .{ .allocate = .alloc_always });
        },
        else => return error.InvalidJwks,
    };
}

fn ensure_etc_hosts_entry(io: std.Io, allocator: mem.Allocator, hostname: []const u8, ipv6: []const u8) !void {
    try append_hosts_entry(io, allocator, "/etc/hosts", hostname, ipv6);
}

fn append_hosts_entry(io: std.Io, allocator: mem.Allocator, hosts_path: []const u8, hostname: []const u8, ipv6: []const u8) !void {
    const existing: []u8 = readFileAlloc(io, allocator, hosts_path) catch |err| switch (err) {
        error.FileNotFound => &[_]u8{},
        else => return err,
    };
    defer if (existing.len > 0) allocator.free(existing);

    if (existing.len > 0 and std.mem.indexOf(u8, existing, hostname) != null) return;

    const needs_newline = existing.len > 0 and existing[existing.len - 1] != '\n';
    const updated = try std.fmt.allocPrint(
        allocator,
        "{s}{s}{s} {s}\n",
        .{ existing, if (needs_newline) "\n" else "", ipv6, hostname },
    );
    defer allocator.free(updated);
    try writeFile(allocator, hosts_path, updated);
}

fn normalize_neon_safekeeper_connstrings(
    io: std.Io,
    allocator: mem.Allocator,
    raw: ?[]const u8,
    alias_prefix: []const u8,
    default_alias: []const u8,
) ![][]const u8 {
    var entries = std.array_list.Managed([]const u8).init(allocator);
    errdefer entries.deinit();

    if (raw) |raw_value| {
        var it = std.mem.splitScalar(u8, raw_value, ',');
        while (it.next()) |entry| {
            const trimmed = std.mem.trim(u8, entry, " \t\r\n");
            if (trimmed.len == 0) continue;
            if (try normalize_neon_safekeeper_connstr(io, allocator, trimmed, alias_prefix)) |normalized| {
                try entries.append(normalized);
            }
        }
    }

    if (entries.items.len == 0) {
        try entries.append(try std.fmt.allocPrint(allocator, "{s}:5454", .{default_alias}));
    }

    return try entries.toOwnedSlice();
}

fn normalize_neon_safekeeper_connstr(
    io: std.Io,
    allocator: mem.Allocator,
    value: []const u8,
    alias_prefix: []const u8,
) !?[]const u8 {
    const trimmed = std.mem.trim(u8, value, " \t\r\n");
    if (trimmed.len == 0) return null;
    if (std.mem.indexOfScalar(u8, trimmed, '=') != null) {
        return try allocator.dupe(u8, trimmed);
    }

    var host: []const u8 = undefined;
    var port: []const u8 = undefined;
    if (trimmed[0] == '[') {
        const close = std.mem.indexOfScalar(u8, trimmed, ']') orelse return null;
        host = trimmed[1..close];
        const rest = trimmed[close + 1 ..];
        if (!std.mem.startsWith(u8, rest, ":")) return null;
        port = std.mem.trim(u8, rest[1..], " \t\r\n");
    } else {
        if (std.mem.count(u8, trimmed, ":") > 1) return null;
        const split = std.mem.lastIndexOfScalar(u8, trimmed, ':') orelse return null;
        host = trimmed[0..split];
        port = std.mem.trim(u8, trimmed[split + 1 ..], " \t\r\n");
    }

    if (host.len == 0 or port.len == 0) return null;
    if (!is_all_digits(port)) return null;

    if (std.mem.indexOfScalar(u8, host, ':') != null) {
        const alias = try neon_host_alias(allocator, alias_prefix, host);
        try append_hosts_entry(io, allocator, "/etc/hosts", alias, host);
        const normalized = try std.fmt.allocPrint(allocator, "{s}:{s}", .{ alias, port });
        return normalized;
    }

    return try std.fmt.allocPrint(allocator, "{s}:{s}", .{ host, port });
}

fn is_all_digits(value: []const u8) bool {
    for (value) |c| {
        if (c < '0' or c > '9') return false;
    }
    return value.len > 0;
}

fn setup_system(allocator: mem.Allocator) !void {
    const hostname = "localhost";
    if (linux.syscall2(.sethostname, @intFromPtr(hostname.ptr), hostname.len) != 0) {
        return error.SetHostnameFailed;
    }
    try ensure_etc_hosts(allocator, hostname);
    try set_link_up("lo", 65536);
}

fn set_link_up(ifname: []const u8, mtu: u32) !void {
    const fd = linux.socket(AF_NETLINK, linux.SOCK.RAW, NETLINK_ROUTE);
    if (fd < 0) return error.SocketFailed;
    const sfd = @as(i32, @intCast(fd));
    defer _ = linux.close(sfd);

    const if_index = try find_interface_index(sfd, ifname);

    var msg: struct {
        nl: nlmsghdr,
        ifi: ifinfomsg,
        attr: rtattr,
        mtu: u32,
    } = .{
        .nl = .{
            .len = @sizeOf(nlmsghdr) + @sizeOf(ifinfomsg) + @sizeOf(rtattr) + 4,
            .type = RTM_NEWLINK,
            .flags = NLM_F_REQUEST | NLM_F_ACK,
            .seq = 1,
            .pid = 0,
        },
        .ifi = .{
            .family = 0,
            .index = if_index,
            .flags = IFF_UP,
            .change = IFF_UP,
        },
        .attr = .{
            .len = @sizeOf(rtattr) + 4,
            .type = IFLA_MTU,
        },
        .mtu = mtu,
    };

    _ = linux.sendto(sfd, mem.asBytes(&msg), msg.nl.len, 0, null, 0);
}

fn find_interface_index(fd: i32, ifname: []const u8) !i32 {
    var msg: struct {
        nl: nlmsghdr,
        ifi: ifinfomsg,
    } = .{
        .nl = .{
            .len = @sizeOf(nlmsghdr) + @sizeOf(ifinfomsg),
            .type = RTM_GETLINK,
            .flags = NLM_F_REQUEST | NLM_F_DUMP,
            .seq = 2,
            .pid = 0,
        },
        .ifi = .{
            .family = 0,
            .index = 0,
            .flags = 0,
            .change = 0,
        },
    };

    _ = linux.sendto(fd, mem.asBytes(&msg), msg.nl.len, 0, null, 0);

    var buffer: [4096]u8 = undefined;
    while (true) {
        const n = linux.recvfrom(fd, &buffer, buffer.len, 0, null, null);
        if (n <= 0) break;

        var offset: usize = 0;
        while (offset + @sizeOf(nlmsghdr) <= n) {
            const h = @as(*const nlmsghdr, @ptrCast(@alignCast(buffer[offset..].ptr)));
            if (h.type == 3) return error.NetlinkError; 
            if (h.type == 2) break; 

            if (h.type == RTM_NEWLINK) {
                const ifi = @as(*const ifinfomsg, @ptrCast(@alignCast(buffer[offset + @sizeOf(nlmsghdr) ..].ptr)));
                
                var attr_offset = offset + @sizeOf(nlmsghdr) + @sizeOf(ifinfomsg);
                const msg_end = offset + h.len;
                while (attr_offset + @sizeOf(rtattr) <= msg_end) {
                    const rta = @as(*const rtattr, @ptrCast(@alignCast(buffer[attr_offset..].ptr)));
                    if (rta.len < @sizeOf(rtattr)) break;
                    if (rta.type == IFLA_IFNAME) {
                        const name = buffer[attr_offset + @sizeOf(rtattr) .. attr_offset + rta.len];
                        const name_trimmed = mem.trim(u8, name, "\x00");
                        if (mem.eql(u8, name_trimmed, ifname)) {
                            return ifi.index;
                        }
                    }
                    attr_offset += (rta.len + 3) & ~@as(u16, 3);
                }
            }
            offset += (h.len + 3) & ~@as(u32, 3);
        }
    }
    return error.InterfaceNotFound;
}

fn ensure_etc_hosts(allocator: mem.Allocator, hostname: []const u8) !void {
    try makePath(allocator, "/etc");
    const hosts_content = try std.fmt.allocPrint(allocator,
        "::1 localhost ip6-localhost ip6-loopback {s}\n",
        .{hostname},
    );
    defer allocator.free(hosts_content);

    if (!pathExists("/etc/hosts")) {
        try writeFile(allocator, "/etc/hosts", hosts_content);
    }
    if (!pathExists("/etc/hostname")) {
        try writeFile(allocator, "/etc/hostname", hostname);
    }
}

fn writeFile(allocator: mem.Allocator, path: []const u8, data: []const u8) !void {
    const path_z = try dupeZ(allocator, path);
    defer allocator.free(path_z);
    const fd = linux.openat(linux.AT.FDCWD, path_z, .{
        .ACCMODE = .WRONLY,
        .CREAT = true,
        .TRUNC = true,
    }, 0o644);
    if (fd < 0) return error.OpenFailed;
    const sfd = @as(i32, @intCast(fd));
    defer _ = linux.close(sfd);
    _ = linux.write(sfd, data.ptr, data.len);
}

fn pathExists(path: []const u8) bool {
    const path_z = dupeZ(std.heap.page_allocator, path) catch return false;
    defer std.heap.page_allocator.free(path_z);
    return linux.access(path_z, 0) == 0;
}

fn load_config(io: std.Io, allocator: mem.Allocator, path: []const u8) !InitConfig {
    const content = try readFileAlloc(io, allocator, path);
    defer allocator.free(content);
    return try std.json.parseFromSliceLeaky(InitConfig, allocator, content, .{
        .ignore_unknown_fields = true,
        .allocate = .alloc_always,
    });
}

fn envGet(env: std.json.Value, key: []const u8) ?[]const u8 {
    if (env != .object) return null;
    const value = env.object.get(key) orelse return null;
    return switch (value) {
        .string => |s| s,
        else => null,
    };
}

fn readFileAlloc(io: std.Io, allocator: mem.Allocator, path: []const u8) ![]u8 {
    const file = try std.Io.Dir.openFileAbsolute(io, path, .{});
    defer file.close(io);

    var reader = file.reader(io, &.{});
    return try reader.interface.allocRemaining(allocator, .unlimited);
}

fn setup_networking(io: std.Io, allocator: mem.Allocator, config: InitConfig) !void {
    const link_name = try detect_network_interface(io, allocator);
    std.debug.print("[mikrom-init] Configuring {s} interface...\n", .{link_name});
    try set_link_up(link_name, 1500);
    try ensure_ipv6_enabled(link_name);

    const fd = linux.socket(AF_NETLINK, linux.SOCK.RAW, NETLINK_ROUTE);
    if (fd < 0) return error.SocketFailed;
    const sfd = @as(i32, @intCast(fd));
    defer _ = linux.close(sfd);
    const if_index = try find_interface_index(sfd, link_name);

    if (envGet(config.env, "IPV6_ADDR")) |ipv6_addr_str| {
        var addr_part = ipv6_addr_str;
        var prefix: u8 = 64;
        if (mem.indexOfScalar(u8, ipv6_addr_str, '/')) |idx| {
            addr_part = ipv6_addr_str[0..idx];
            prefix = try std.fmt.parseInt(u8, ipv6_addr_str[idx + 1 ..], 10);
        }

        const ipv6 = try net.Ip6Address.parse(addr_part, 0);
        try add_ipv6_addr(sfd, @as(u32, @intCast(if_index)), ipv6.bytes, prefix);
    }

    if (envGet(config.env, "IPV6_GW")) |gw_str| {
        const gw = try net.Ip6Address.parse(gw_str, 0);
        var zero_addr: [16]u8 = undefined;
        @memset(&zero_addr, 0);
        try add_ipv6_route(sfd, @as(u32, @intCast(if_index)), zero_addr, 0, gw.bytes);

        if (config.workload_type == .DATABASE) {
            const pageserver_ipv6 = try net.Ip6Address.parse(
                envGet(config.env, "NEON_PAGESERVER_IPV6") orelse DEFAULT_NEON_PAGESERVER_IPV6,
                0,
            );
            std.debug.print(
                "[mikrom-init] Adding explicit route to Neon pageserver {any} via {s}\n",
                .{ pageserver_ipv6, link_name },
            );
            try add_ipv6_route(sfd, @as(u32, @intCast(if_index)), pageserver_ipv6.bytes, 128, gw.bytes);
        }
    }

    try configure_resolver(config);
}

fn detect_network_interface(io: std.Io, allocator: mem.Allocator) ![]u8 {
    var dir = try std.Io.Dir.openDirAbsolute(io, "/sys/class/net", .{ .iterate = true });
    defer dir.close(io);

    var candidates = std.array_list.Managed([]const u8).init(std.heap.page_allocator);
    defer candidates.deinit();

    var it = dir.iterate();
    while (try it.next(io)) |entry| {
        const name = entry.name;
        if (std.mem.eql(u8, name, "lo") or
            std.mem.eql(u8, name, "sit0") or
            std.mem.eql(u8, name, "tunl0") or
            std.mem.startsWith(u8, name, "gre"))
        {
            continue;
        }
        try candidates.append(try std.heap.page_allocator.dupe(u8, name));
    }

    if (candidates.items.len == 0) return error.NoNetworkInterface;
    std.mem.sort([]const u8, candidates.items, {}, struct {
        fn lessThan(_: void, a: []const u8, b: []const u8) bool {
            return std.mem.lessThan(u8, a, b);
        }
    }.lessThan);
    const selected = try allocator.dupe(u8, candidates.items[0]);
    for (candidates.items) |item| std.heap.page_allocator.free(item);
    return selected;
}

fn ensure_ipv6_enabled(link_name: []const u8) !void {
    const disable_ipv6_path = try std.fmt.allocPrint(std.heap.page_allocator, "/proc/sys/net/ipv6/conf/{s}/disable_ipv6", .{link_name});
    defer std.heap.page_allocator.free(disable_ipv6_path);
    if (pathExists(disable_ipv6_path)) {
        writeFile(std.heap.page_allocator, disable_ipv6_path, "0") catch |err| {
            std.debug.print("[mikrom-init] Warning: Failed to enable IPv6 on {s}: {any}\n", .{ link_name, err });
        };
    }
}

fn add_ipv6_addr(fd: i32, if_index: u32, addr: [16]u8, prefix: u8) !void {
    var msg: struct {
        nl: nlmsghdr,
        ifa: ifaddrmsg,
        attr: rtattr,
        addr: [16]u8,
    } = .{
        .nl = .{
            .len = @sizeOf(nlmsghdr) + @sizeOf(ifaddrmsg) + @sizeOf(rtattr) + 16,
            .type = RTM_NEWADDR,
            .flags = NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            .seq = 3,
            .pid = 0,
        },
        .ifa = .{
            .family = 10,
            .prefixlen = prefix,
            .flags = 0,
            .scope = 0,
            .index = if_index,
        },
        .attr = .{
            .len = @sizeOf(rtattr) + 16,
            .type = IFA_LOCAL,
        },
        .addr = addr,
    };
    _ = linux.sendto(fd, mem.asBytes(&msg), msg.nl.len, 0, null, 0);
}

fn add_ipv6_route(fd: i32, if_index: u32, destination: [16]u8, prefix: u8, gateway: [16]u8) !void {
    var msg: struct {
        nl: nlmsghdr,
        rt: rtmsg,
        attr_dst: rtattr,
        dst: [16]u8,
        attr_gw: rtattr,
        gw: [16]u8,
        attr_oif: rtattr,
        oif: u32,
    } = .{
        .nl = .{
            .len = @sizeOf(nlmsghdr) + @sizeOf(rtmsg) + (@sizeOf(rtattr) + 16) + (@sizeOf(rtattr) + 16) + (@sizeOf(rtattr) + 4),
            .type = RTM_NEWROUTE,
            .flags = NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            .seq = 4,
            .pid = 0,
        },
        .rt = .{
            .family = 10,
            .dst_len = prefix,
            .src_len = 0,
            .tos = 0,
            .table = 254,
            .protocol = 3,
            .scope = 0,
            .type = 1,
            .flags = 0,
        },
        .attr_dst = .{
            .len = @sizeOf(rtattr) + 16,
            .type = RTA_DST,
        },
        .dst = destination,
        .attr_gw = .{
            .len = @sizeOf(rtattr) + 16,
            .type = RTA_GATEWAY,
        },
        .gw = gateway,
        .attr_oif = .{
            .len = @sizeOf(rtattr) + 4,
            .type = RTA_OIF,
        },
        .oif = if_index,
    };
    _ = linux.sendto(fd, mem.asBytes(&msg), msg.nl.len, 0, null, 0);
}

fn configure_resolver(config: InitConfig) !void {
    const dns_server = envGet(config.env, "DNS_SERVER") orelse "fd00::3bc2:7b88:289:62e6";
    const resolv_conf = try std.fmt.allocPrint(
        std.heap.page_allocator,
        "nameserver {s}\nsearch mikrom.internal\noptions ndots:1 timeout:1 attempts:2\n",
        .{dns_server},
    );
    defer std.heap.page_allocator.free(resolv_conf);
    try writeFile(std.heap.page_allocator, "/etc/resolv.conf", resolv_conf);
}

fn dump_pgdata_state(io: std.Io, path: []const u8) void {
    std.debug.print("[mikrom-init] Inspecting {s} after database failure...\n", .{path});
    const dir = std.Io.Dir.openDirAbsolute(io, path, .{ .iterate = true }) catch |err| {
        std.debug.print("[mikrom-init]   unable to read {s}: {any}\n", .{ path, err });
        return;
    };
    defer dir.close(io);

    var it = dir.iterate();
    while (true) {
        const next = it.next(io) catch |err| {
            std.debug.print("[mikrom-init]   unable to iterate {s}: {any}\n", .{ path, err });
            return;
        };
        const entry = next orelse break;
        switch (entry.kind) {
            .directory => std.debug.print("[mikrom-init]   {s} (dir)\n", .{entry.name}),
            .sym_link => {
                var link_buf: [256]u8 = undefined;
                const n = dir.readLink(io, entry.name, &link_buf) catch {
                    std.debug.print("[mikrom-init]   {s} (symlink -> <unreadable>)\n", .{entry.name});
                    continue;
                };
                std.debug.print("[mikrom-init]   {s} (symlink -> {s})\n", .{ entry.name, link_buf[0..n] });
            },
            .file => std.debug.print("[mikrom-init]   {s} (file)\n", .{entry.name}),
            else => std.debug.print("[mikrom-init]   {s} (other)\n", .{entry.name}),
        }
    }
}

fn setup_volume_mounts(io: std.Io, allocator: mem.Allocator, config: InitConfig) !void {
    if (config.volumes.len == 0) return;
    std.debug.print("[mikrom-init] Setting up volume mounts...\n", .{});

    for (config.volumes) |vol| {
        const device = find_device_by_serial(io, allocator, vol.drive_id) catch |err| {
            if (vol.index) |idx| {
                const letter = @as(u8, @intCast('a' + idx));
                const dev = try std.fmt.allocPrint(allocator, "/dev/vd{c}", .{letter});
                std.debug.print("[mikrom-init] Serial discovery failed for {s}, mapped by index {d} -> {s}\n", .{ vol.drive_id, idx, dev });
                try mount_volume_device(allocator, dev, vol.mount_point);
                continue;
            } else {
                std.debug.print("[mikrom-init] Warning: Device not found for volume {s}: {any}\n", .{vol.drive_id, err});
                continue;
            }
        };
        try mount_volume_device(allocator, device, vol.mount_point);
    }
}

fn mount_volume_device(allocator: mem.Allocator, device: []const u8, mount_point: []const u8) !void {
    std.debug.print("[mikrom-init] Mounting {s} to {s}...\n", .{ device, mount_point });

    if (!deviceNodeExists(device)) {
        std.debug.print(
            "[mikrom-init] Warning: Device node {s} does not exist in /dev, wait-and-retry...\n",
            .{device},
        );
        const ts = std.posix.timespec{
            .sec = 0,
            .nsec = @as(isize, @intCast(500 * std.time.ns_per_ms)),
        };
        _ = std.c.nanosleep(&ts, null);
    }

    try mount_fs(allocator, device, mount_point, "ext4", 0);
}

fn deviceNodeExists(device: []const u8) bool {
    const device_z = dupeZ(std.heap.page_allocator, device) catch return false;
    defer std.heap.page_allocator.free(device_z);
    return linux.access(device_z, 0) == 0;
}

fn find_device_by_serial(io: std.Io, allocator: mem.Allocator, drive_id: []const u8) ![]const u8 {
    const target_serial = if (drive_id.len > 20) drive_id[0..20] else drive_id;

    var attempt: usize = 0;
    while (attempt < 10) : (attempt += 1) {
        if (attempt > 0) {
            const ts = std.posix.timespec{
                .sec = 0,
                .nsec = @as(isize, @intCast(500 * std.time.ns_per_ms)),
            };
            _ = linux.nanosleep(&ts, null);
        }

        var dir = std.Io.Dir.openDirAbsolute(io, "/sys/block", .{ .iterate = true }) catch |err| {
            std.debug.print("[mikrom-init] Warning: Failed to read /sys/block: {any}\n", .{err});
            continue;
        };
        defer dir.close(io);

        var it = dir.iterate();
        while (try it.next(io)) |entry| {
            if (!mem.startsWith(u8, entry.name, "vd")) continue;

            var dev_dir = try dir.openDir(io, entry.name, .{});
            defer dev_dir.close(io);

            const serial = dev_dir.readFileAlloc(io, "serial", allocator, .unlimited) catch continue;
            defer allocator.free(serial);

            const trimmed = mem.trim(u8, serial, " \n\r\t");
            if (mem.eql(u8, trimmed, target_serial) or mem.startsWith(u8, drive_id, trimmed)) {
                return try std.fmt.allocPrint(allocator, "/dev/{s}", .{entry.name});
            }
        }
    }
    return error.DeviceNotFound;
}

fn start_background_services(io: std.Io, allocator: mem.Allocator) !void {
    const sshd_path = "/usr/sbin/sshd";
    const path_z = try dupeZ(allocator, sshd_path);
    defer allocator.free(path_z);
    const fd = linux.openat(linux.AT.FDCWD, path_z, .{ .ACCMODE = .RDONLY }, 0);
    if (fd >= 0) {
        _ = linux.close(@as(i32, @intCast(fd)));
        std.debug.print("[mikrom-init] Starting SSH daemon...\n", .{});
        try makePath(allocator, "/run/sshd");
        _ = try std.process.spawn(io, .{
            .argv = &.{sshd_path},
        });
    }
}

fn spawn_application(io: std.Io, allocator: mem.Allocator, base_env: *const std.process.Environ.Map, config: InitConfig) !std.process.Child {
    var env_map = try base_env.clone(allocator);
    defer env_map.deinit();
    switch (config.env) {
        .object => |obj| {
            var it = obj.iterator();
            while (it.next()) |entry| {
                if (entry.value_ptr.* != .string) continue;
                try env_map.put(entry.key_ptr.*, entry.value_ptr.string);
            }
        },
        else => return error.InvalidEnvFormat,
    }

    const mikrom_user = try resolve_mikrom_user(io, allocator);
    const path_env = env_map.get("PATH");
    const effective = try effective_path(allocator, path_env);
    defer allocator.free(effective);
    try env_map.put("PATH", effective);
    try env_map.put("HOME", mikrom_user.dir);
    try env_map.put("USER", mikrom_user.name);
    try env_map.put("LOGNAME", mikrom_user.name);

    try makePath(allocator, config.workdir);
    
    var argv = std.array_list.Managed([]const u8).init(allocator);
    defer argv.deinit();

    if (config.entrypoint.len > 0) {
        try argv.appendSlice(config.entrypoint);
        try argv.appendSlice(config.cmd);
    } else if (config.cmd.len > 0) {
        try argv.appendSlice(config.cmd);
    } else {
        return error.NoEntrypointOrCmd;
    }

    return try std.process.spawn(io, .{
        .argv = argv.items,
        .cwd = .{ .path = config.workdir },
        .environ_map = &env_map,
        .uid = mikrom_user.uid,
        .gid = mikrom_user.gid,
    });
}

fn effective_path(allocator: mem.Allocator, existing: ?[]const u8) ![]u8 {
    var parts = std.array_list.Managed([]const u8).init(allocator);
    defer parts.deinit();

    try append_unique_path(&parts, "/app/node_modules/.bin");
    try append_unique_path(&parts, "/mise/shims");
    try append_unique_path(&parts, "/usr/local/bin");

    if (existing) |path| {
        var it = mem.splitScalar(u8, path, ':');
        while (it.next()) |part| {
            if (part.len == 0) continue;
            try append_unique_path(&parts, part);
        }
    }

    try append_unique_path(&parts, "/usr/local/sbin");
    try append_unique_path(&parts, "/usr/sbin");
    try append_unique_path(&parts, "/usr/bin");
    try append_unique_path(&parts, "/sbin");
    try append_unique_path(&parts, "/bin");

    var list = std.array_list.Managed(u8).init(allocator);
    errdefer list.deinit();

    for (parts.items, 0..) |part, idx| {
        if (idx > 0) try list.append(':');
        try list.appendSlice(part);
    }

    return try list.toOwnedSlice();
}

fn append_unique_path(parts: anytype, part: []const u8) !void {
    for (parts.items) |existing| {
        if (mem.eql(u8, existing, part)) return;
    }
    try parts.append(part);
}

const UserInfo = struct {
    uid: u32,
    gid: u32,
    dir: []const u8,
    name: []const u8,
};

fn resolve_mikrom_user(io: std.Io, allocator: mem.Allocator) !UserInfo {
    const content = try readFileAlloc(io, allocator, "/etc/passwd");
    defer allocator.free(content);
    var line_it = mem.splitScalar(u8, content, '\n');

    while (line_it.next()) |line| {
        var it = mem.splitScalar(u8, line, ':');
        const name = it.next() orelse continue;
        if (mem.eql(u8, name, "mikrom")) {
            _ = it.next();
            const uid_str = it.next() orelse continue;
            const gid_str = it.next() orelse continue;
            _ = it.next();
            const dir = it.next() orelse continue;
            
            return UserInfo{
                .uid = try std.fmt.parseInt(u32, uid_str, 10),
                .gid = try std.fmt.parseInt(u32, gid_str, 10),
                .dir = try dupe(allocator, dir),
                .name = try dupe(allocator, name),
            };
        }
    }
    return error.UserNotFound;
}

fn wait_for_port_ready(io: std.Io, port: u16, child: *std.process.Child) !void {
    var attempts: usize = 0;
    while (attempts < 150) : (attempts += 1) {
        if (child.id) |pid| {
            var status: i32 = 0;
            if (linux.waitpid(pid, &status, linux.W.NOHANG) == @as(usize, @intCast(pid))) {
                return error.ApplicationExitedEarly;
            }
        }
        if (try_connect(io, port, "127.0.0.1")) return;
        if (try_connect(io, port, "::1")) return;
        const ts = std.posix.timespec{
            .sec = 0,
            .nsec = @as(isize, @intCast(200 * std.time.ns_per_ms)),
        };
        _ = std.c.nanosleep(&ts, null);
    }
    return error.Timeout;
}

fn try_connect(io: std.Io, port: u16, addr_str: []const u8) bool {
    const addr = net.IpAddress.parse(addr_str, port) catch return false;
    const stream = addr.connect(io, .{ .mode = .stream }) catch return false;
    stream.close(io);
    return true;
}

fn fallback_to_shell() noreturn {
    std.debug.print("[mikrom-init] Falling back to {s}...\n", .{FALLBACK_SHELL});
    const argv = [_:null]?[*:0]const u8{
        @ptrCast(FALLBACK_SHELL.ptr),
        null,
    };
    const envp = [_:null]?[*:0]const u8{null};
    _ = linux.execve(@ptrCast(FALLBACK_SHELL.ptr), &argv, &envp);
    std.debug.print("[mikrom-init] CRITICAL: All execution attempts failed. Halting.\n", .{});
    while (true) _ = linux.pause();
}

test "effective_path prepends and deduplicates" {
    const allocator = std.testing.allocator;
    const path = try effective_path(allocator, "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin:/custom/bin");
    defer allocator.free(path);

    try std.testing.expect(std.mem.startsWith(u8, path, "/app/node_modules/.bin:/mise/shims:/usr/local/bin"));
    try std.testing.expect(std.mem.indexOf(u8, path, "/usr/local/bin:/usr/local/bin") == null);
    try std.testing.expect(std.mem.indexOf(u8, path, "/custom/bin") != null);
}

test "effective_path defaults when missing" {
    const allocator = std.testing.allocator;
    const path = try effective_path(allocator, null);
    defer allocator.free(path);

    try std.testing.expect(std.mem.startsWith(u8, path, "/app/node_modules/.bin:/mise/shims:/usr/local/bin"));
    try std.testing.expect(std.mem.indexOf(u8, path, "/usr/local/sbin") != null);
    try std.testing.expect(std.mem.indexOf(u8, path, "/bin") != null);
}

test "config deserializes workload type and defaults" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();

    const json = "{\"entrypoint\":[\"/bin/sh\"],\"workload_type\":\"DATABASE\"}";
    const config = try std.json.parseFromSliceLeaky(InitConfig, arena.allocator(), json, .{
        .ignore_unknown_fields = true,
        .allocate = .alloc_always,
    });

    try std.testing.expectEqual(WorkloadType.DATABASE, config.workload_type);
    try std.testing.expectEqualStrings("/app", config.workdir);
    try std.testing.expectEqual(@as(usize, 1), config.entrypoint.len);
    try std.testing.expectEqual(@as(usize, 0), config.cmd.len);
    try std.testing.expectEqual(@as(usize, 0), config.volumes.len);
}

test "parse_bool_flag accepts common representations" {
    try std.testing.expect(try parse_bool_flag("true"));
    try std.testing.expect(try parse_bool_flag("ON"));
    try std.testing.expect(!(try parse_bool_flag("0")));
    try std.testing.expect(!(try parse_bool_flag("off")));
}

test "neon_host_alias normalizes punctuation" {
    const allocator = std.testing.allocator;
    const alias = try neon_host_alias(allocator, "neon-pageserver", "fd00::dead:beef");
    defer allocator.free(alias);

    try std.testing.expectEqualStrings("neon-pageserver-fd00--dead-beef", alias);
}
