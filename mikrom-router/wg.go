package mikrom

import (
	"fmt"
	"os/exec"
	"strings"

	"go.uber.org/zap"
)

type WireGuardManager struct {
	Interface string
	Logger    *zap.Logger

	privKey string
	pubKey  string
	ipv6    string
}

func NewWireGuardManager(iface string, logger *zap.Logger) *WireGuardManager {
	return &WireGuardManager{
		Interface: iface,
		Logger:    logger,
	}
}

func (w *WireGuardManager) Init(hostID, ipv6 string) (string, error) {
	w.ipv6 = ipv6

	// 1. Ensure config dirs exist
	exec.Command("sudo", "mkdir", "-p", "/etc/mikrom").Run()
	exec.Command("sudo", "mkdir", "-p", "/etc/wireguard").Run()

	keyPath := "/etc/mikrom/router.key"
	pubKeyPath := "/etc/mikrom/router.pub"

	// 2. Check if key exists using sudo
	err := exec.Command("sudo", "test", "-f", keyPath).Run()
	if err != nil {
		w.Logger.Info("generating new WireGuard keypair")
		out, err := exec.Command("wg", "genkey").Output()
		if err != nil {
			return "", fmt.Errorf("failed to generate key: %w", err)
		}
		privKey := strings.TrimSpace(string(out))

		// Write private key using sudo
		cmd := exec.Command("sudo", "tee", keyPath)
		cmd.Stdin = strings.NewReader(privKey)
		if err := cmd.Run(); err != nil {
			return "", fmt.Errorf("failed to write private key: %w", err)
		}
		exec.Command("sudo", "chmod", "600", keyPath).Run()

		// Generate and write pubkey
		cmd = exec.Command("wg", "pubkey")
		cmd.Stdin = strings.NewReader(privKey)
		out, err = cmd.Output()
		if err != nil {
			return "", fmt.Errorf("failed to generate pubkey: %w", err)
		}
		pubKey := strings.TrimSpace(string(out))
		cmd = exec.Command("sudo", "tee", pubKeyPath)
		cmd.Stdin = strings.NewReader(pubKey)
		cmd.Run()
	}

	// 3. Read and cache keys
	out, err := exec.Command("sudo", "cat", keyPath).Output()
	if err != nil {
		return "", fmt.Errorf("failed to read private key: %w", err)
	}
	w.privKey = strings.TrimSpace(string(out))

	out, err = exec.Command("sudo", "cat", pubKeyPath).Output()
	if err != nil {
		return "", fmt.Errorf("failed to read public key: %w", err)
	}
	w.pubKey = strings.TrimSpace(string(out))

	// 4. Create initial wg-quick config using sudo
	conf := fmt.Sprintf(`[Interface]
PrivateKey = %s
Address = %s/64
ListenPort = 51822
PostUp = ip -6 route add fd00::/8 dev %%i metric 100 || true
PostUp = ip -6 route add fd0d::/16 dev %%i metric 10 || true
`, w.privKey, w.ipv6)

	confPath := fmt.Sprintf("/etc/wireguard/%s.conf", w.Interface)
	cmd := exec.Command("sudo", "tee", confPath)
	cmd.Stdin = strings.NewReader(conf)
	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("failed to write wg config: %w", err)
	}

	// 5. Restart interface with wg-quick
	exec.Command("sudo", "wg-quick", "down", w.Interface).Run()
	if out, err := exec.Command("sudo", "wg-quick", "up", w.Interface).CombinedOutput(); err != nil {
		w.Logger.Warn("wg-quick up failed (might already be up)", zap.Error(err), zap.String("output", string(out)))
	}

	w.Logger.Info("WireGuard interface initialized via wg-quick", zap.String("interface", w.Interface), zap.String("ip", w.ipv6))
	return w.pubKey, nil
}

func (w *WireGuardManager) UpdatePeers(peers []PeerInfo) error {
	if w.privKey == "" || w.pubKey == "" || w.ipv6 == "" {
		return fmt.Errorf("WireGuardManager not initialized")
	}

	// 1. Build config using cached state
	var sb strings.Builder
	sb.WriteString("[Interface]\n")
	sb.WriteString(fmt.Sprintf("PrivateKey = %s\n", w.privKey))
	sb.WriteString(fmt.Sprintf("Address = %s/64\n", w.ipv6))
	sb.WriteString("ListenPort = 51822\n")
	sb.WriteString("PostUp = ip -6 route add fd00::/8 dev %i metric 100 || true\n")
	sb.WriteString("PostUp = ip -6 route add fd0d::/16 dev %i metric 10 || true\n\n")

	for _, peer := range peers {
		if peer.PublicKey == "" || peer.Endpoint == "" || peer.PublicKey == w.pubKey {
			continue
		}

		allowedIPs := strings.Join(peer.AllowedIPs, ",")
		if allowedIPs == "" {
			allowedIPs = "fd00::/8"
		}

		sb.WriteString("[Peer]\n")
		sb.WriteString(fmt.Sprintf("PublicKey = %s\n", peer.PublicKey))
		sb.WriteString(fmt.Sprintf("Endpoint = %s\n", peer.Endpoint))
		sb.WriteString(fmt.Sprintf("AllowedIPs = %s\n", allowedIPs))
		sb.WriteString("PersistentKeepalive = 25\n\n")
	}

	// 2. Write config using sudo
	confPath := fmt.Sprintf("/etc/wireguard/%s.conf", w.Interface)
	cmd := exec.Command("sudo", "tee", confPath)
	cmd.Stdin = strings.NewReader(sb.String())
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("failed to write wg config: %w", err)
	}

	// 3. Sync configuration
	stripCmd := fmt.Sprintf("wg-quick strip %s | wg syncconf %s /dev/stdin", w.Interface, w.Interface)
	if out, err := exec.Command("sudo", "bash", "-c", stripCmd).CombinedOutput(); err != nil {
		w.Logger.Warn("failed to sync WG configuration", zap.Error(err), zap.String("output", string(out)))
	}

	w.Logger.Info("WireGuard mesh updated via wg syncconf", zap.Int("peers", len(peers)))
	return nil
}

type PeerInfo struct {
	HostID     string
	Endpoint   string
	PublicKey  string
	AllowedIPs []string
}
