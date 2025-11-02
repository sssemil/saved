use crate::view::NetworkView;
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;

/// Manages relay peer reservations with RTT-based pruning strategy.
///
/// Policy: Reserve relays opportunistically, keep at most N reservations,
/// preferring those with active traffic and lowest RTT.
pub struct RelayManager {
    /// FIFO queue of reserved relay peers (for iteration order)
    reserved: VecDeque<PeerId>,
    /// Set of relays currently carrying active traffic
    in_use: HashSet<PeerId>,
    /// Maximum number of concurrent relay reservations
    max_relays: usize,
}

impl RelayManager {
    /// Create a new relay manager with the specified max relay count.
    pub fn new(max_relays: usize) -> Self {
        Self {
            reserved: VecDeque::new(),
            in_use: HashSet::new(),
            max_relays,
        }
    }

    /// Check if we can accept a new relay reservation.
    /// Returns true if we have capacity or if this relay is already reserved.
    pub fn can_reserve(&self, relay_id: &PeerId) -> bool {
        // Already reserved - always ok
        if self.reserved.contains(relay_id) {
            return true;
        }

        // Check if we have capacity for a new relay
        self.reserved.len() < (self.in_use.len() + self.max_relays)
    }

    /// Register a new relay reservation (idempotent).
    pub fn register(&mut self, relay_id: PeerId) {
        if !self.reserved.contains(&relay_id) {
            self.reserved.push_back(relay_id);
        }
    }

    /// Recompute which relays are currently in use by scanning the network view.
    pub fn recompute_in_use(&mut self, view: &NetworkView) {
        // TODO: Can replace this with just keeping an up-to-date version based on changes
        self.in_use.clear();
        for info in view.peers.values() {
            for meta in &info.addresses {
                if !meta.active_connections.is_empty()
                    && Self::addr_is_relay(&meta.address)
                    && let Some(relay_id) = Self::extract_relay_peer(&meta.address)
                {
                    self.in_use.insert(relay_id);
                }
            }
        }
    }

    /// Check if we need to prune (have more reservations than allowed).
    ///
    /// Policy: Keep all in-use relays + max_relays spare capacity.
    /// Never prune a relay that's actively carrying traffic.
    pub fn should_prune(&self) -> bool {
        self.reserved.len() > (self.in_use.len() + self.max_relays)
    }

    /// Get the worst (highest RTT) unused relay to prune, if any.
    ///
    /// Returns None if all reserved relays are in use (should not prune).
    pub fn get_worst_unused(&self, view: &NetworkView) -> Option<PeerId> {
        self.reserved
            .iter()
            .filter(|r| !self.in_use.contains(r))
            .max_by_key(|relay_id| {
                // Get RTT for this relay, default to max duration if unknown
                view.peers
                    .get(relay_id)
                    .map(|p| p.rtt)
                    .unwrap_or(Duration::MAX)
            })
            .copied()
    }

    /// Remove a relay from the reserved set (e.g., after pruning).
    pub fn remove(&mut self, relay_id: &PeerId) -> bool {
        if let Some(idx) = self.reserved.iter().position(|r| r == relay_id) {
            self.reserved.remove(idx);
            true
        } else {
            false
        }
    }

    /// Get the RTT for a relay from the view, or None if unknown.
    pub fn get_relay_rtt(&self, view: &NetworkView, relay_id: &PeerId) -> Option<Duration> {
        view.peers.get(relay_id).map(|p| p.rtt)
    }

    /// Check if a multiaddr contains a relay circuit.
    pub fn addr_is_relay(addr: &Multiaddr) -> bool {
        addr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
    }

    /// Extract the relay PeerId from a relayed multiaddr like
    /// /.../p2p/<relay>/p2p-circuit(/p2p/<dest>)...
    pub fn extract_relay_peer(addr: &Multiaddr) -> Option<PeerId> {
        let mut last_p2p = None;
        for p in addr.iter() {
            match p {
                Protocol::P2p(peer_id) => last_p2p = Some(peer_id),
                Protocol::P2pCircuit => return last_p2p,
                _ => {}
            }
        }
        None
    }

    /// Get current statistics for debugging.
    pub fn stats(&self) -> RelayStats {
        RelayStats {
            reserved_count: self.reserved.len(),
            in_use_count: self.in_use.len(),
            max_relays: self.max_relays,
        }
    }
}

impl Default for RelayManager {
    fn default() -> Self {
        Self::new(2)
    }
}

/// Statistics about current relay state.
#[derive(Debug, Clone)]
pub struct RelayStats {
    pub reserved_count: usize,
    pub in_use_count: usize,
    pub max_relays: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_idempotent() {
        let mut manager = RelayManager::new(2);
        let relay = PeerId::random();

        manager.register(relay);
        assert_eq!(manager.reserved.len(), 1);

        manager.register(relay);
        assert_eq!(manager.reserved.len(), 1, "should not add duplicate");
    }

    #[test]
    fn test_should_prune() {
        let mut manager = RelayManager::new(2);

        assert!(!manager.should_prune());

        manager.register(PeerId::random());
        assert!(!manager.should_prune());

        manager.register(PeerId::random());
        assert!(!manager.should_prune());

        manager.register(PeerId::random());
        assert!(manager.should_prune(), "should prune when over max");
    }

    #[test]
    fn test_remove() {
        let mut manager = RelayManager::new(2);
        let relay = PeerId::random();

        manager.register(relay);
        assert!(manager.remove(&relay));
        assert!(!manager.remove(&relay), "second remove should return false");
    }

    #[test]
    fn test_addr_is_relay() {
        let relay_id = PeerId::random();
        let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{}/p2p-circuit", relay_id)
            .parse()
            .unwrap();
        let normal_addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001".parse().unwrap();

        assert!(RelayManager::addr_is_relay(&relay_addr));
        assert!(!RelayManager::addr_is_relay(&normal_addr));
    }

    #[test]
    fn test_extract_relay_peer() {
        let relay_id = PeerId::random();
        let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{}/p2p-circuit", relay_id)
            .parse()
            .unwrap();

        let extracted = RelayManager::extract_relay_peer(&relay_addr);
        assert_eq!(extracted, Some(relay_id));
    }
}
