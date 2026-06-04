# NAT64/DNS64 Smoke Checklist

Use this checklist after deploying `mikrom-agent` and `mikrom-dns` on a worker node.

## 1. Verify Host Services

On the compute node:

```bash
sudo systemctl status mikrom-agent --no-pager
sudo ip link show tundra
sudo ip -6 addr show dev tundra
sudo ip -6 route show | grep '64:ff9b::/96'
sudo ip6tables -t nat -S
```

Expected:

- `mikrom-agent` is `active (running)`.
- `tundra` exists and is `UP`.
- `tundra` has `192.168.64.254/24` and `fd00:6464::fffe/64`.
- A single route exists for `64:ff9b::/96` via `tundra`.
- `ip6tables` shows one `SNAT` rule for `64:ff9b::/96` and one `MASQUERADE` rule for `fd00::/64`.

## 2. Verify DNS64

From a VM on that node:

```bash
cat /etc/resolv.conf
dig AAAA ipv4only.arpa @fd00::3bc2:7b88:289:62e6
```

Expected:

- `resolv.conf` points to `nameserver fd00::3bc2:7b88:289:62e6`.
- `dig` returns synthesized AAAA records:
  - `64:ff9b::c000:aa`
  - `64:ff9b::c000:ab`

## 3. Verify NAT64 End-to-End

From the same VM:

```bash
openssl s_client -connect [64:ff9b::101:101]:443 -servername one.one.one.one </dev/null
```

Expected:

- The connection reaches `64:ff9b::101:101:443`.
- TLS handshake succeeds.
- The certificate chain is returned and verification completes.

## 4. Cleanup Check

After restarting `mikrom-agent` once or twice:

```bash
sudo systemctl restart mikrom-agent
sudo ip6tables -t nat -S
sudo journalctl -u mikrom-agent -b --no-pager | grep NAT64
```

Expected:

- The NAT64 rules do not duplicate across restarts.
- Logs show NAT64 translator startup without leftover-process warnings.

## 5. If Something Fails

- If `tundra` is missing, check `journalctl -u mikrom-agent -b --no-pager | grep NAT64`.
- If DNS64 does not synthesize AAAA, verify `mikrom-dns` is running on the node and listening on UDP/53.
- If DNS64 works but NAT64 fails, verify the `ip6tables` rules and the `64:ff9b::/96` route on the host.

