use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::ConnectionId;
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct NetworkView {
    pub peers: HashMap<PeerId, PeerInfoExt>,
}

#[derive(Debug, Default)]
pub struct PeerInfoExt {
    /// Known or advertised addresses for this peer (kept exactly as observed, including /p2p/...),
    /// plus per-address connection data and minimal metadata for sorting.
    /// TODO: Monitor for slowdowns with this approach, too many O(n) shenanigans rn
    pub addresses: Vec<AddressMetadata>,
    protocols: HashSet<StreamProtocol>,
    pub supports_relay_hop_v2: bool,
    pub rtt: Duration,
}

trait Ranked {
    fn rank(&self) -> u8;
}

/// Transport classes we care about for ranking. (Treat WS/WSS as Other.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Transport {
    Quic,
    Tcp,
    Other,
    Relay,
}

impl Ranked for Transport {
    /// Smaller rank = higher preference (for sorting).
    fn rank(&self) -> u8 {
        match self {
            Transport::Quic => 0,
            Transport::Tcp => 1,
            Transport::Other => 2,
            Transport::Relay => 3,
        }
    }
}

/// Coarse network scope for locality-aware preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetScope {
    LinkLocal,  // v4 169.254/16, v6 fe80::/10
    PrivateLan, // v4 RFC1918, v6 ULA fc00::/7
    Public,
}

impl Ranked for NetScope {
    fn rank(&self) -> u8 {
        match self {
            NetScope::PrivateLan => 0,
            NetScope::LinkLocal => 1,
            NetScope::Public => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddressMetadata {
    /// The multiaddr as observed (kept exactly, including any /p2p segments).
    /// TODO: Consider removing the p2p segment
    /// TODO: See what the case with relays is
    pub address: Multiaddr,
    /// All active connections currently using this remote addr.
    pub active_connections: Vec<ConnectionId>,
    pub transport: Transport,
    pub scope: NetScope,
    pub last_err: Option<Instant>,
    pub err_count: u32,
}

impl Default for AddressMetadata {
    fn default() -> Self {
        Self {
            address: Multiaddr::empty(),
            active_connections: Vec::new(),
            transport: Transport::Other,
            scope: NetScope::Public,
            last_err: None,
            err_count: 0,
        }
    }
}

impl AddressMetadata {
    fn is_in_cooldown(&self) -> bool {
        let now = Instant::now();
        if let Some(at) = self.last_err {
            let backoff = Duration::from_secs(1 << self.err_count.min(6)); // 1..64s
            return now.duration_since(at) < backoff;
        }
        false
    }
}

impl NetworkView {
    pub fn add_address(&mut self, peer_id: PeerId, addr: Multiaddr) -> bool {
        // Keep the address exactly as reported (including any /p2p segments).
        let info = self.peers.entry(peer_id).or_default();

        // O(n) upsert: if present, do nothing; else push a freshly classified metadata entry.
        if info.addresses.iter().any(|m| m.address == addr) {
            // still keep the vector sorted to maintain invariant
            info.resort();
            return false;
        }

        let (scope, transport) = classify_addr(&addr);
        info.addresses.push(AddressMetadata {
            address: addr,
            transport,
            scope,
            ..Default::default()
        });

        // Always keep best-first ordering.
        info.resort();

        true
    }

    pub fn remove_address(&mut self, peer_id: &PeerId, addr: &Multiaddr) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            if let Some(pos) = info.addresses.iter().position(|m| &m.address == addr) {
                info.addresses.remove(pos);
            }
            // Always keep best-first ordering.
            info.resort();
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

        // If this conn_id already exists on a different address, move it.
        if let Some((idx_old, _)) = info
            .addresses
            .iter_mut()
            .enumerate()
            .find(|(_, m)| m.active_connections.contains(&conn_id))
        {
            if info.addresses[idx_old].address != remote {
                if let Some(pos) = info.addresses[idx_old]
                    .active_connections
                    .iter()
                    .position(|c| *c == conn_id)
                {
                    info.addresses[idx_old].active_connections.swap_remove(pos);
                }
            } else {
                // Already tracked on the same address; success clears backoff and return.
                let oa = &info.addresses[idx_old].address.clone();
                info.clear_backoff_on_success(oa);
                info.resort();
                return;
            }
        }

        // Ensure we have metadata for the remote address (Observed classification if unseen).
        if !info.addresses.iter().any(|m| m.address == remote) {
            let (scope, transport) = classify_addr(&remote);
            info.addresses.push(AddressMetadata {
                address: remote.clone(),
                transport,
                scope,
                ..Default::default()
            });
        }

        // Append connection id to the matching address' active list and clear backoff on success.
        if let Some(meta) = info.addresses.iter_mut().find(|m| m.address == remote)
            && !meta.active_connections.contains(&conn_id)
        {
            meta.active_connections.push(conn_id);
        }
        info.clear_backoff_on_success(&remote);

        // Always keep best-first ordering.
        info.resort();
    }

    pub fn remove_connection(&mut self, peer_id: &PeerId, conn_id: ConnectionId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            // Find and remove this connection ID from whichever address holds it.
            for meta in &mut info.addresses {
                if let Some(pos) = meta.active_connections.iter().position(|c| *c == conn_id) {
                    meta.active_connections.swap_remove(pos);
                    break;
                }
            }
            // Always keep best-first ordering.
            info.resort();
            self.prune_empty(peer_id);
        }
    }

    pub fn is_connected_over(&self, peer_id: &PeerId, addr: &Multiaddr) -> bool {
        self.peers
            .get(peer_id)
            .and_then(|info| {
                info.addresses
                    .iter()
                    .find(|m| &m.address == addr)
                    .map(|m| !m.active_connections.is_empty())
            })
            .unwrap_or(false)
    }

    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|p| p.addresses.iter().any(|m| !m.active_connections.is_empty()))
            .unwrap_or(false)
    }

    fn prune_empty(&mut self, peer_id: &PeerId) {
        let remove = if let Some(info) = self.peers.get(peer_id) {
            let no_conns = info
                .addresses
                .iter()
                .all(|m| m.active_connections.is_empty());
            no_conns && info.addresses.is_empty()
        } else {
            false
        };
        if remove {
            self.peers.remove(peer_id);
        }
    }

    pub fn set_protocols(&mut self, peer_id: PeerId, protocols: Vec<StreamProtocol>) {
        let protocols: HashSet<StreamProtocol> = HashSet::from_iter(protocols);
        // detect HOP v2 support
        let hop_supported = protocols.iter().any(|p| {
            let s = p.as_ref();
            s == "/libp2p/circuit/relay/0.2.0/hop" || s == "/libp2p/circuit/relay/0.2.0"
        });

        let p = self.peers.entry(peer_id).or_default();

        // Show new and removed protocols if any
        let removed_protocols = p.protocols.difference(&protocols);
        let added_protocols = protocols.difference(&p.protocols);
        for protocol in removed_protocols {
            println!("[{peer_id}] PR: -{:?}", protocol);
        }
        for protocol in added_protocols {
            println!("[{peer_id}] PR: +{:?}", protocol);
        }

        p.protocols = protocols;
        p.supports_relay_hop_v2 = hop_supported;
    }
}

impl PeerInfoExt {
    /// Helper used by callers that need to know which remote address a given connection uses.
    pub(crate) fn get_connection_address(&self, connection_id: &ConnectionId) -> Option<Multiaddr> {
        self.addresses
            .iter()
            .find(|m| m.active_connections.contains(connection_id))
            .map(|m| m.address.clone())
    }

    /// Mark a failure for an address: bump exponential backoff and send it to the end.
    ///
    /// Backoff schedule: 1, 2, 4, 8, 16, 32, 64 seconds (capped), during which the
    /// address is considered "cooling down" and sorted behind all non-cooldown entries.
    pub fn mark_failure(&mut self, addr: &Multiaddr) {
        if let Some(m) = self.addresses.iter_mut().find(|m| &m.address == addr) {
            m.err_count = m.err_count.saturating_add(1);
            m.last_err = Some(Instant::now());
            // After marking failure, immediately push it down by re-sorting.
            self.resort();
        }
    }

    /// On successful use of an address, clear any backoff state.
    pub fn clear_backoff_on_success(&mut self, addr: &Multiaddr) {
        if let Some(m) = self.addresses.iter_mut().find(|m| &m.address == addr) {
            m.err_count = 0;
            m.last_err = None;
        }
    }

    /// Keep the addresses vector sorted so that index 0 is always the current "best" candidate.
    ///
    /// Unless under backoff, the priority is:
    /// 1) Address scope (PrivateLan > LinkLocal > Public)
    /// 2) Transport (QUIC > TCP > Other)
    /// 3) Resolution (IP over DNS)
    ///
    /// If an address is in exponential backoff cooldown (recent error not yet cooled down),
    /// it is sorted to the **end** regardless of the other criteria.
    fn resort(&mut self) {
        self.addresses.sort_by_key(|m| {
            let in_cooldown = m.is_in_cooldown(); // true -> send to end
            (
                in_cooldown,
                m.scope.rank(),     // smaller rank is better
                m.transport.rank(), // smaller rank is better
                m.address.clone(),
            )
        });
    }
}

fn classify_addr(addr: &Multiaddr) -> (NetScope, Transport) {
    let mut scope = NetScope::Public;
    let mut transport = Transport::Other;

    for p in addr.iter() {
        match p {
            Protocol::Ip4(ip) => {
                scope = match ip.octets() {
                    // RFC1918
                    [10, ..] => NetScope::PrivateLan,
                    [172, b, ..] if (16..=31).contains(&b) => NetScope::PrivateLan,
                    [192, 168, ..] => NetScope::PrivateLan,
                    // Link-local v4
                    [169, 254, ..] => NetScope::LinkLocal,
                    _ => NetScope::Public,
                }
            }
            Protocol::Ip6(ip) => {
                let seg0 = u16::from_be_bytes([ip.octets()[0], ip.octets()[1]]);
                scope = if (seg0 & 0xfe00) == 0xfc00 {
                    NetScope::PrivateLan // fc00::/7 ULA
                } else if (seg0 & 0xffc0) == 0xfe80 {
                    NetScope::LinkLocal // fe80::/10
                } else {
                    NetScope::Public
                };
            }
            Protocol::QuicV1 | Protocol::Quic => transport = Transport::Quic,
            Protocol::Tcp(_) => {
                if transport.rank() > Transport::Tcp.rank() {
                    transport = Transport::Tcp
                }
            }
            Protocol::P2pCircuit => {
                transport = Transport::Relay;
            }
            _ => {}
        }
    }
    (scope, transport)
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

    fn vec_contains_addr(v: &[AddressMetadata], addr: &Multiaddr) -> bool {
        v.iter().any(|m| &m.address == addr)
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
            vec_contains_addr(&info.addresses, &with_p2p_suffix),
            "we keep the exact addr with /p2p"
        );
        assert!(
            !vec_contains_addr(&info.addresses, &base),
            "we do not auto-normalize to a version without /p2p"
        );
    }

    #[test]
    fn add_and_remove_address_exact() {
        let mut view = NetworkView::default();
        let peer = PeerId::random();
        let a = ip4_tcp([10, 0, 0, 2], 3333);

        view.add_address(peer, a.clone());
        assert!(vec_contains_addr(
            &view.peers.get(&peer).unwrap().addresses,
            &a
        ));

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

        // Sanity: two connections tracked across the two addresses
        let info = view.peers.get(&peer).unwrap();
        let total_conns: usize = info
            .addresses
            .iter()
            .map(|m| m.active_connections.len())
            .sum();
        assert_eq!(total_conns, 2);

        // get_connection_address works
        let addr1 = info
            .get_connection_address(&ConnectionId::new_unchecked(1))
            .unwrap();
        assert_eq!(addr1, remote1);
        let addr2 = info
            .get_connection_address(&ConnectionId::new_unchecked(2))
            .unwrap();
        assert_eq!(addr2, send_back);
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
            view.peers.contains_key(&peer),
            "peer NOT pruned after last connection removed"
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
