const std = @import("std");
const posix = std.posix;

pub fn main() !void {
    std.debug.print("posix available\n", .{});
}
