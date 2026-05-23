const std = @import("std");
const mem = std.mem;
const net = std.Io.net;
const linux = std.os.linux;

const CONFIG_PATH = "/etc/mikrom/init.json";
const FALLBACK_SHELL = "/bin/sh";

pub const VolumeConfig = struct {
    drive_id: []const u8,
    mount_point: []const u8,
    index: ?usize = null,
};

pub const InitConfig = struct {
    env: std.json.Value = .{ .object = .empty },
    workdir: []const u8 = "/app",
    entrypoint: [][]const u8,
    cmd: [][]const u8 = &.{},
    volumes: []VolumeConfig = &.{},
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

    setup_networking(config) catch |err| {
        std.debug.print("[mikrom-init] Warning: Networking setup encountered errors: {any}\n", .{err});
    };

    setup_volume_mounts(init.io, allocator, config) catch |err| {
        std.debug.print("[mikrom-init] Warning: Volume mounting encountered errors: {any}\n", .{err});
    };

    start_background_services(init.io, allocator) catch |err| {
        std.debug.print("[mikrom-init] Warning: Failed to start background services: {any}\n", .{err});
    };

    std.debug.print("[mikrom-init] Starting application: {any}\n", .{config.entrypoint});
    
    const port_str = envGet(config.env, "PORT") orelse "8080";
    const port = std.fmt.parseInt(u16, port_str, 10) catch 8080;

    var child = try spawn_application(init.io, allocator, init.environ_map, config);
    
    wait_for_port_ready(init.io, port, &child) catch |err| {
        std.debug.print("[mikrom-init] Application never became ready: {any}\n", .{err});
        fallback_to_shell();
    };

    std.debug.print("__MIKROM_APP_START__\n", .{});

    const term = try child.wait(init.io);
    std.debug.print("[mikrom-init] Application exited with {any}\n", .{term});

    fallback_to_shell();
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
        "127.0.0.1 localhost\n::1 localhost ip6-localhost ip6-loopback\n127.0.1.1 {s}\n", 
        .{hostname});
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
    const parsed = try std.json.parseFromSlice(InitConfig, allocator, content, .{
        .ignore_unknown_fields = true,
    });
    return parsed.value;
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

fn setup_networking(config: InitConfig) !void {
    std.debug.print("[mikrom-init] Configuring eth0 interface...\n", .{});
    try set_link_up("eth0", 1500);

    if (envGet(config.env, "IPV6_ADDR")) |ipv6_addr_str| {
        const fd = linux.socket(AF_NETLINK, linux.SOCK.RAW, NETLINK_ROUTE);
        if (fd < 0) return error.SocketFailed;
        const sfd = @as(i32, @intCast(fd));
        defer _ = linux.close(sfd);
        const if_index = try find_interface_index(sfd, "eth0");

        var addr_part = ipv6_addr_str;
        var prefix: u8 = 64;
        if (mem.indexOfScalar(u8, ipv6_addr_str, '/')) |idx| {
            addr_part = ipv6_addr_str[0..idx];
            prefix = try std.fmt.parseInt(u8, ipv6_addr_str[idx+1..], 10);
        }

        const ipv6 = try net.Ip6Address.parse(addr_part, 0);
        try add_ipv6_addr(sfd, @as(u32, @intCast(if_index)), ipv6.bytes, prefix);

        if (envGet(config.env, "IPV6_GW")) |gw_str| {
            const gw = try net.Ip6Address.parse(gw_str, 0);
            try add_ipv6_route(sfd, @as(u32, @intCast(if_index)), gw.bytes);
        }
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

fn add_ipv6_route(fd: i32, if_index: u32, gateway: [16]u8) !void {
    var msg: struct {
        nl: nlmsghdr,
        rt: rtmsg,
        attr_gw: rtattr,
        gw: [16]u8,
        attr_oif: rtattr,
        oif: u32,
    } = .{
        .nl = .{
            .len = @sizeOf(nlmsghdr) + @sizeOf(rtmsg) + (@sizeOf(rtattr) + 16) + (@sizeOf(rtattr) + 4),
            .type = RTM_NEWROUTE,
            .flags = NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            .seq = 4,
            .pid = 0,
        },
        .rt = .{
            .family = 10,
            .dst_len = 0,
            .src_len = 0,
            .tos = 0,
            .table = 254,
            .protocol = 3,
            .scope = 0,
            .type = 1,
            .flags = 0,
        },
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
    
    var dir = try std.Io.Dir.openDirAbsolute(io, "/sys/block", .{ .iterate = true });
    defer dir.close(io);

    var it = dir.iterate();
    while (try it.next(io)) |entry| {
        if (!mem.startsWith(u8, entry.name, "vd")) continue;

        var dev_dir = try dir.openDir(io, entry.name, .{});
        defer dev_dir.close(io);

        const serial = try dev_dir.readFileAlloc(io, "serial", allocator, .unlimited);
        defer allocator.free(serial);
        
        const trimmed = mem.trim(u8, serial, " \n\r\t");
        if (mem.eql(u8, trimmed, target_serial)) {
            return try std.fmt.allocPrint(allocator, "/dev/{s}", .{entry.name});
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
