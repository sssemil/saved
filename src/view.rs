use libp2p::core::ConnectedPoint;
use libp2p::swarm::ConnectionId;
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub struct NetworkView {
    pub peers: HashMap<PeerId, PeerInfoExt>,
}

#[derive(Debug, Default)]
pub struct PeerInfoExt {
    /// Known or advertised addresses for this peer (kept exactly as observed, including /p2p/...).
    pub addresses: HashSet<Multiaddr>,
    /// Active connections keyed by ConnectionId -> remote addr (kept exactly as observed).
    pub connections: HashMap<ConnectionId, Multiaddr>,
    /// How many active connections currently use a given remote addr.
    active_addrs: HashMap<Multiaddr, u32>,
    protocols: Vec<StreamProtocol>,
    pub supports_relay_hop_v2: bool,
}

impl NetworkView {
    pub fn add_address(&mut self, peer_id: PeerId, addr: Multiaddr) {
        // Keep the address exactly as reported (including any /p2p segments).
        self.peers
            .entry(peer_id)
            .or_default()
            .addresses
            .insert(addr);
    }

    pub fn remove_address(&mut self, peer_id: &PeerId, addr: &Multiaddr) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.addresses.remove(addr);
            self.prune_empty(peer_id);
        }
    }

    pub fn add_connection(
        &mut self,
        peer_id: PeerId,
        conn_id: ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        // Keep the address exactly as reported by the endpoint.
        let remote = match endpoint {
            ConnectedPoint::Dialer { address, .. } => address.clone(),
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr.clone(),
        };

        let info = self.peers.entry(peer_id).or_default();

        // If this conn_id already exists, do nothing (or update if the addr changed).
        if let Some(prev) = info.connections.insert(conn_id, remote.clone()) {
            // If the address changed (should be rare), fix refcounts.
            if prev != remote {
                dec_addr_ref(&mut info.active_addrs, &prev);
                inc_addr_ref(&mut info.active_addrs, remote);
            }
            return;
        }

        // New connectionId -> addr mapping.
        inc_addr_ref(&mut info.active_addrs, remote);
    }

    pub fn remove_connection(&mut self, peer_id: &PeerId, conn_id: ConnectionId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            if let Some(addr) = info.connections.remove(&conn_id) {
                dec_addr_ref(&mut info.active_addrs, &addr);
            }
            self.prune_empty(peer_id);
        }
    }

    pub fn is_connected_over(&self, peer_id: &PeerId, addr: &Multiaddr) -> bool {
        self.peers
            .get(peer_id)
            .and_then(|info| info.active_addrs.get(addr))
            .map(|&n| n > 0)
            .unwrap_or(false)
    }

    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|p| !p.connections.is_empty())
            .unwrap_or(false)
    }

    fn prune_empty(&mut self, peer_id: &PeerId) {
        let remove = if let Some(info) = self.peers.get(peer_id) {
            info.connections.is_empty() && info.addresses.is_empty()
        } else {
            false
        };
        if remove {
            self.peers.remove(peer_id);
        }
    }

    pub fn add_protocols(&mut self, peer_id: PeerId, mut protocols: Vec<StreamProtocol>) {
        for p in &mut protocols {
            println!("\t[{peer_id}] Adding protocol {p:?}");
        }

        // detect HOP v2 support
        let hop_supported = protocols.iter().any(|p| {
            let s = p.as_ref();
            s == "/libp2p/circuit/relay/0.2.0/hop" || s == "/libp2p/circuit/relay/0.2.0"
        });

        let p = self.peers.entry(peer_id).or_default();
        p.protocols.append(&mut protocols);
        p.supports_relay_hop_v2 = hop_supported;
    }
}

fn inc_addr_ref(map: &mut HashMap<Multiaddr, u32>, addr: Multiaddr) {
    *map.entry(addr).or_insert(0) += 1;
}

fn dec_addr_ref(map: &mut HashMap<Multiaddr, u32>, addr: &Multiaddr) {
    if let Some(cnt) = map.get_mut(addr) {
        *cnt -= 1;
        if *cnt == 0 {
            map.remove(addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::core::ConnectedPoint;
    use libp2p::multiaddr::Protocol;
    use libp2p::swarm::ConnectionId;
    use libp2p::{Multiaddr, PeerId};
    use std::net::Ipv4Addr;

    fn ip4_tcp(ip: [u8; 4], port: u16) -> Multiaddr {
        let mut a = Multiaddr::empty();
        a.push(Protocol::Ip4(Ipv4Addr::from(ip)));
        a.push(Protocol::Tcp(port));
        a
    }

    fn with_p2p(mut a: Multiaddr, peer: &PeerId) -> Multiaddr {
        a.push(Protocol::P2p(*peer));
        a
    }

    fn relayed_addr(base_ip: [u8; 4], port: u16, relay: &PeerId, dest: &PeerId) -> Multiaddr {
        let mut a = ip4_tcp(base_ip, port);
        a.push(Protocol::P2p(*relay));
        a.push(Protocol::P2pCircuit);
        a.push(Protocol::P2p(*dest));
        a
    }

    #[test]
    fn add_address_keeps_p2p_suffix() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();

        let base = ip4_tcp([127, 0, 0, 1], 4001);
        let with_p2p_suffix = with_p2p(base.clone(), &peer);

        view.add_address(peer, with_p2p_suffix.clone());

        let info = view.peers.get(&peer).expect("peer present");
        assert!(
            info.addresses.contains(&with_p2p_suffix),
            "we keep the exact addr with /p2p"
        );
        assert!(
            !info.addresses.contains(&base),
            "we do not auto-normalize to a version without /p2p"
        );
    }

    #[test]
    fn add_and_remove_address_exact() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();
        let a = ip4_tcp([10, 0, 0, 2], 3333);

        view.add_address(peer, a.clone());
        assert!(view.peers.get(&peer).unwrap().addresses.contains(&a));

        view.remove_address(&peer, &a);
        assert!(
            !view.peers.contains_key(&peer),
            "peer entry pruned when empty"
        );
    }

    #[test]
    fn add_connection_dialer_and_listener_exact() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();
        let relay = PeerId::random();

        // Dialer case uses `address`
        let remote1 = relayed_addr([192, 168, 1, 5], 4001, &relay, &peer);
        let cp1 = ConnectedPoint::Dialer {
            address: remote1.clone(),
            role_override: libp2p::core::Endpoint::Dialer,
            port_use: libp2p::core::transport::PortUse::New,
        };
        view.add_connection(peer, ConnectionId::new_unchecked(1), &cp1);
        assert!(view.is_connected_over(&peer, &remote1));

        // Listener case uses `send_back_addr`
        let relay2 = PeerId::random();
        let send_back = relayed_addr([10, 0, 0, 7], 7007, &relay2, &peer);
        let cp2 = ConnectedPoint::Listener {
            local_addr: ip4_tcp([0, 0, 0, 0], 0),
            send_back_addr: send_back.clone(),
        };
        view.add_connection(peer, ConnectionId::new_unchecked(2), &cp2);
        assert!(view.is_connected_over(&peer, &send_back));

        // Sanity: two connections tracked
        let info = view.peers.get(&peer).unwrap();
        assert_eq!(info.connections.len(), 2);
    }

    #[test]
    fn remove_connection_updates_flags_exact() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();
        let relay = PeerId::random();

        let remote = relayed_addr([203, 0, 113, 9], 5555, &relay, &peer);
        let cp = ConnectedPoint::Dialer {
            address: remote.clone(),
            role_override: libp2p::core::Endpoint::Dialer,
            port_use: libp2p::core::transport::PortUse::New,
        };

        view.add_connection(peer, ConnectionId::new_unchecked(10), &cp);
        assert!(view.is_connected_over(&peer, &remote));

        view.remove_connection(&peer, ConnectionId::new_unchecked(10));
        assert!(!view.is_connected_over(&peer, &remote));
        assert!(
            !view.peers.contains_key(&peer),
            "peer pruned after last connection removed"
        );
    }

    #[test]
    fn multiple_connections_same_addr_refcounted_exact() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();
        let relay = PeerId::random();

        let remote = relayed_addr([100, 64, 0, 1], 4242, &relay, &peer);
        let cp = ConnectedPoint::Dialer {
            address: remote.clone(),
            role_override: libp2p::core::Endpoint::Dialer,
            port_use: libp2p::core::transport::PortUse::New,
        };

        // Add two different connection ids to the same addr
        view.add_connection(peer, ConnectionId::new_unchecked(1), &cp);
        view.add_connection(peer, ConnectionId::new_unchecked(2), &cp);

        assert!(view.is_connected_over(&peer, &remote));

        // Remove one, still connected
        view.remove_connection(&peer, ConnectionId::new_unchecked(1));
        assert!(view.is_connected_over(&peer, &remote));

        // Remove the second, no longer connected
        view.remove_connection(&peer, ConnectionId::new_unchecked(2));
        assert!(!view.is_connected_over(&peer, &remote));
    }

    #[test]
    fn address_change_on_same_connection_updates_refcounts_exact() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();
        let relay_a = PeerId::random();
        let relay_b = PeerId::random();

        let a1 = relayed_addr([1, 2, 3, 4], 1111, &relay_a, &peer);
        let a2 = relayed_addr([5, 6, 7, 8], 2222, &relay_b, &peer);

        // Start as Dialer on a1
        let cp1 = ConnectedPoint::Dialer {
            address: a1.clone(),
            role_override: libp2p::core::Endpoint::Dialer,
            port_use: libp2p::core::transport::PortUse::New,
        };
        view.add_connection(peer, ConnectionId::new_unchecked(77), &cp1);
        assert!(view.is_connected_over(&peer, &a1));
        assert!(!view.is_connected_over(&peer, &a2));

        // Simulate same ConnectionId now associated with a2 (e.g., migration)
        let cp2 = ConnectedPoint::Dialer {
            address: a2.clone(),
            role_override: libp2p::core::Endpoint::Dialer,
            port_use: libp2p::core::transport::PortUse::New,
        };
        view.add_connection(peer, ConnectionId::new_unchecked(77), &cp2);

        assert!(!view.is_connected_over(&peer, &a1));
        assert!(view.is_connected_over(&peer, &a2));
    }
}
