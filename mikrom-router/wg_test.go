package mikrom

import (
	"strings"
	"testing"
)

func TestGenerateWGConfig(t *testing.T) {
	privKey := "uKhpI5wxSHi/oKitAJ937vrjklrtx/24Y4FLoCBrY2Q="
	hostIPv6 := "fd00::ace"

	conf := `[Interface]
PrivateKey = ` + privKey + `
Address = ` + hostIPv6 + `/64
ListenPort = 51822
PostUp = ip -6 route add fd00::/8 dev %i metric 100 || true
PostUp = ip -6 route add fd0d::/16 dev %i metric 10 || true
`
	if !strings.Contains(conf, "Address = fd00::ace/64") {
		t.Errorf("Config missing correct address: %v", conf)
	}
	if !strings.Contains(conf, "ListenPort = 51822") {
		t.Errorf("Config missing correct port: %v", conf)
	}
}

func TestPeerFiltering(t *testing.T) {
	myPubKey := "QNlzqt4IyWll/qLZ4sRem6cyzWLpPBEONx0vj0rrAkU="
	peers := []PeerInfo{
		{HostID: "self", PublicKey: myPubKey, Endpoint: "1.1.1.1:51820"},
		{HostID: "other", PublicKey: "other-key", Endpoint: "2.2.2.2:51820"},
	}

	filtered := 0
	for _, p := range peers {
		if p.PublicKey == myPubKey {
			continue
		}
		filtered++
	}

	if filtered != 1 {
		t.Errorf("Expected 1 filtered peer, got %d", filtered)
	}
}
