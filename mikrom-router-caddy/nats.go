package mikrom

import (
	"fmt"
	"strings"

	routerv1 "github.com/antpard/mikrom/mikrom-proto/proto/router/v1"
	schedulerv1 "github.com/antpard/mikrom/mikrom-proto/proto/scheduler/v1"
	"github.com/nats-io/nats.go"
	"go.uber.org/zap"
	"google.golang.org/protobuf/proto"
)

const (
	SubjectRouterConfigUpdated        = "mikrom.router.config_updated"
	SubjectRouterTlsCertUpdated       = "mikrom.router.tls_cert_updated"
	SubjectRouterAcmeChallengeUpdated = "mikrom.router.acme_challenge_updated"
	SubjectSchedulerNetworkMesh       = "mikrom.scheduler.network.mesh"
)

func (m *MikromApp) handleMeshUpdate(data []byte) {
	var update schedulerv1.NetworkMeshUpdate
	if err := proto.Unmarshal(data, &update); err != nil {
		m.logger.Error("failed to unmarshal network mesh update", zap.Error(err))
		return
	}

	var peers []PeerInfo
	for _, p := range update.Peers {
		if p.Endpoint == "" {
			continue
		}
		peers = append(peers, PeerInfo{
			HostID:     p.HostId,
			Endpoint:   fmt.Sprintf("%s:%d", p.Endpoint, p.WireguardPort),
			PublicKey:  p.WireguardPubkey,
			AllowedIPs: p.AllowedIps,
		})

	}

	if err := m.wg.UpdatePeers(peers); err != nil {
		m.logger.Error("failed to update WireGuard peers", zap.Error(err))
	}
}

func (m *MikromApp) listenForUpdates(hostID string) {
	subject := fmt.Sprintf("mikrom.scheduler.network.mesh.%s", hostID)

	m.nc.Subscribe(subject, func(msg *nats.Msg) {
		m.handleMeshUpdate(msg.Data)
	})

	m.nc.Subscribe(SubjectRouterConfigUpdated, func(msg *nats.Msg) {
		var update routerv1.RouterConfigUpdate
		if err := proto.Unmarshal(msg.Data, &update); err != nil {
			m.logger.Error("failed to unmarshal router config update", zap.Error(err))
			return
		}

		if update.TargetUrl != nil {
			target := *update.TargetUrl
			// Strip http:// or https:// before storing to optimize hot-path
			target = strings.TrimPrefix(target, "http://")
			target = strings.TrimPrefix(target, "https://")

			m.routes.Store(update.Hostname, target)
			if err := m.saveRoute(update.Hostname, target); err != nil {
				m.logger.Error("failed to save route to DB", zap.Error(err))
			}
		} else {
			m.routes.Delete(update.Hostname)
			if err := m.deleteRoute(update.Hostname); err != nil {
				m.logger.Error("failed to delete route from DB", zap.Error(err))
			}
		}
	})

	m.nc.Subscribe(SubjectRouterAcmeChallengeUpdated, func(msg *nats.Msg) {
		var update routerv1.AcmeChallengeUpdate
		if err := proto.Unmarshal(msg.Data, &update); err != nil {
			m.logger.Error("failed to unmarshal acme challenge update", zap.Error(err))
			return
		}

		if update.IsDelete {
			m.acme.Delete(update.Token)
			if err := m.deleteACMEChallenge(update.Token); err != nil {
				m.logger.Error("failed to delete acme challenge from DB", zap.Error(err))
			}
		} else {
			m.acme.Store(update.Token, update.KeyAuth)
			if err := m.saveACMEChallenge(update.Token, update.KeyAuth, update.Hostname); err != nil {
				m.logger.Error("failed to save acme challenge to DB", zap.Error(err))
			}
		}
	})

	m.nc.Subscribe(SubjectRouterTlsCertUpdated, func(msg *nats.Msg) {
		var update routerv1.TlsCertificateUpdate
		if err := proto.Unmarshal(msg.Data, &update); err != nil {
			m.logger.Error("failed to unmarshal tls cert update", zap.Error(err))
			return
		}

		if err := m.saveTLSCertificate(update.Hostname, update.CertChain, update.PrivateKey, update.ExpiresAt); err != nil {
			m.logger.Error("failed to save tls certificate to DB", zap.Error(err))
		}

		// Decrypt and load into Caddy's certificate cache if needed
		// For now we just save to DB as Caddy can manage its own certificates via ACME
		// but if we want to support uploaded certificates, we would decrypt here.
		if m.MasterKey != "" {
			_, err := decrypt(update.PrivateKey, m.MasterKey)
			if err != nil {
				m.logger.Error("failed to decrypt private key", zap.String("hostname", update.Hostname), zap.Error(err))
			} else {
				m.logger.Info("received and decrypted certificate for", zap.String("hostname", update.Hostname))
				// TODO: Load into Caddy's certmagic
			}
		}
	})
}
