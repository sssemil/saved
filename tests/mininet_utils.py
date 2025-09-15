#!/usr/bin/env python3
"""
Mininet helpers to build common test topologies:
- Simple LAN with two hosts and a switch
- Double-NAT topology with two private hosts behind separate NAT routers and a public internet host

These helpers expose utilities to configure NAT behavior (e.g., symmetric NAT) and
block/allow new UDP flows to simulate traversal success or failure.

All functions return a started Mininet instance and a dictionary of named hosts/nodes.
"""

from __future__ import annotations

import time
from typing import Dict, Tuple

from mininet.net import Mininet
from mininet.node import Node, Host, OVSSwitch
from mininet.link import TCLink
from mininet.topo import Topo


class LinuxRouter(Node):
	"""A Node with IP forwarding enabled, suitable for use as a router/NAT."""

	def config(self, **params):  # type: ignore[no-untyped-def]
		super().config(**params)
		self.cmd('sysctl -w net.ipv4.ip_forward=1')

	def terminate(self):  # type: ignore[no-untyped-def]
		self.cmd('sysctl -w net.ipv4.ip_forward=0')
		super().terminate()


def _setup_nat_rules(router: Node, inside_if: str, outside_if: str, symmetric: bool = False, block_new_udp: bool = False) -> None:
	"""Configure iptables NAT and forwarding policies on router."""

	masq_flag = '--random-fully' if symmetric else ''
	router.cmd(f'iptables -t nat -F')
	router.cmd(f'iptables -F')
	# Default allow established/related
	router.cmd(f'iptables -A FORWARD -i {outside_if} -o {inside_if} -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT')
	# Allow outbound from inside to outside
	router.cmd(f'iptables -A FORWARD -i {inside_if} -o {outside_if} -j ACCEPT')
	# Optional: make direct punching harder by dropping brand-new inbound UDP
	if block_new_udp:
		router.cmd(f'iptables -A FORWARD -i {outside_if} -o {inside_if} -p udp -m conntrack --ctstate NEW -j DROP')
	# NAT masquerade on outside interface
	router.cmd(f'iptables -t nat -A POSTROUTING -o {outside_if} -j MASQUERADE {masq_flag}')
	# Tweak conntrack UDP timeouts for quicker tests
	router.cmd('sysctl -w net.netfilter.nf_conntrack_udp_timeout=15')
	router.cmd('sysctl -w net.netfilter.nf_conntrack_udp_timeout_stream=60')


def build_lan_topology() -> Tuple[Mininet, Dict[str, Host]]:
	"""Create a simple LAN: h1 -- s1 -- h2

	Returns: (net, nodes) where nodes has keys: h1, h2, s1
	"""

	net = Mininet(link=TCLink, switch=OVSSwitch, controller=None, autoSetMacs=True, autoStaticArp=True)
	s1 = net.addSwitch('s1')
	h1 = net.addHost('h1', ip='10.0.0.1/24')
	h2 = net.addHost('h2', ip='10.0.0.2/24')
	net.addLink(h1, s1)
	net.addLink(h2, s1)
	net.start()
	# Ensure hosts see each other via ARP
	h1.cmd('ip neigh flush all || true')
	h2.cmd('ip neigh flush all || true')
	return net, {'h1': h1, 'h2': h2, 's1': s1}


def build_double_nat_topology(symmetric: bool = False, block_new_udp: bool = False) -> Tuple[Mininet, Dict[str, Node]]:
	"""Create a double-NAT topology with an internet segment:

	Private nets:
	  h1(10.0.1.10) -- r1(10.0.1.1 | 100.64.0.1) -- s0 -- r2(100.64.0.2 | 10.0.2.1) -- h2(10.0.2.10)
	Also attach an internet host hinet(100.64.0.100) to s0 for rendezvous/relay services.

	Args:
	  symmetric: when True, randomize NAT ports (harder punching)
	  block_new_udp: when True, drop new inbound UDP on routers (forces relay fallback)

	Returns: (net, nodes) mapping keys: h1, r1, h2, r2, hinet, s0
	"""

	net = Mininet(link=TCLink, switch=OVSSwitch, controller=None, autoSetMacs=True, autoStaticArp=True)
	s0 = net.addSwitch('s0')

	# Routers
	r1 = net.addHost('r1', cls=LinuxRouter, ip='10.0.1.1/24')
	r2 = net.addHost('r2', cls=LinuxRouter, ip='10.0.2.1/24')

	# Hosts
	h1 = net.addHost('h1', ip='10.0.1.10/24', defaultRoute='via 10.0.1.1')
	h2 = net.addHost('h2', ip='10.0.2.10/24', defaultRoute='via 10.0.2.1')
	hinet = net.addHost('hinet', ip='100.64.0.100/24')

	# Links: h1<->r1, h2<->r2
	net.addLink(h1, r1, intfName2='r1-eth0')
	net.addLink(h2, r2, intfName2='r2-eth0')

	# Outside links: r1<->s0, r2<->s0, hinet<->s0
	net.addLink(r1, s0, intfName1='r1-eth1')
	net.addLink(r2, s0, intfName1='r2-eth1')
	net.addLink(hinet, s0)

	net.start()

	# Configure router IPs
	r1.cmd('ip addr flush dev r1-eth0')
	r1.cmd('ip addr flush dev r1-eth1')
	r1.cmd('ip addr add 10.0.1.1/24 dev r1-eth0')
	r1.cmd('ip addr add 100.64.0.1/24 dev r1-eth1')
	r1.cmd('ip link set r1-eth0 up')
	r1.cmd('ip link set r1-eth1 up')

	r2.cmd('ip addr flush dev r2-eth0')
	r2.cmd('ip addr flush dev r2-eth1')
	r2.cmd('ip addr add 10.0.2.1/24 dev r2-eth0')
	r2.cmd('ip addr add 100.64.0.2/24 dev r2-eth1')
	r2.cmd('ip link set r2-eth0 up')
	r2.cmd('ip link set r2-eth1 up')

	# Internet host IP (link up ensured by Mininet)
	hinet_intf = [i for i in hinet.intfList() if i.name.startswith('hinet-')][0]
	hinet.cmd(f'ip addr flush dev {hinet_intf}')
	hinet.cmd(f'ip addr add 100.64.0.100/24 dev {hinet_intf}')
	hinet.cmd(f'ip link set {hinet_intf} up')

	# Default routes on routers towards outside
	r1.cmd('ip route add default via 100.64.0.254 dev r1-eth1 || true')  # not strictly needed
	r2.cmd('ip route add default via 100.64.0.254 dev r2-eth1 || true')  # placeholder

	# NAT rules
	_setup_nat_rules(r1, inside_if='r1-eth0', outside_if='r1-eth1', symmetric=symmetric, block_new_udp=block_new_udp)
	_setup_nat_rules(r2, inside_if='r2-eth0', outside_if='r2-eth1', symmetric=symmetric, block_new_udp=block_new_udp)

	# Ensure hosts know their default gateway
	h1.cmd('ip route replace default via 10.0.1.1 dev h1-eth0')
	h2.cmd('ip route replace default via 10.0.2.1 dev h2-eth0')

	# Small pause to ensure rules are applied
	time.sleep(0.2)

	return net, {'h1': h1, 'r1': r1, 'h2': h2, 'r2': r2, 'hinet': hinet, 's0': s0}


def run_on(host: Host, cmd: str, background: bool = False):
	"""Run a shell command on a Mininet host, optionally in background returning Popen."""
	if background:
		return host.popen(cmd)
	return host.cmd(cmd)


def wait_procs(procs, timeout: float = 20.0) -> Dict[str, int]:
	"""Wait for a dict of name->Popen, return name->returncode; kill on timeout."""
	end = time.time() + timeout
	ret = {}
	for name, p in procs.items():
		remaining = max(0, end - time.time())
		try:
			code = p.wait(timeout=remaining)
		except Exception:
			p.kill()
			code = -9
		ret[name] = code
	return ret

