#!/usr/bin/env python3
import time

import pytest

from mininet_utils import build_double_nat_topology, run_on, wait_procs


@pytest.fixture(scope="function")
def nat_net_symmetric_false():
	# Easier NAT for punching success
	net, nodes = build_double_nat_topology(symmetric=False, block_new_udp=False)
	try:
		yield net, nodes
	finally:
		net.stop()


@pytest.fixture(scope="function")
def nat_net_symmetric_true_block_new():
	# Hard NAT to force relay fallback
	net, nodes = build_double_nat_topology(symmetric=True, block_new_udp=True)
	try:
		yield net, nodes
	finally:
		net.stop()


def test_udp_hole_punching_success(nat_net_symmetric_false):
	net, nodes = nat_net_symmetric_false
	h1 = nodes['h1']
	h2 = nodes['h2']
	hinet = nodes['hinet']

	# Start rendezvous server on public internet host
	srv = run_on(hinet, 'python3 tests/udp_traversal.py server --bind-host 100.64.0.100 --bind-port 40000 --timeout 15', background=True)
	time.sleep(0.5)

	# Start both clients behind different NATs
	c1 = run_on(h1, 'python3 tests/udp_traversal.py client --server-host 100.64.0.100 --server-port 40000 --my-id A --peer-id B --direct-window 5 --timeout 10', background=True)
	c2 = run_on(h2, 'python3 tests/udp_traversal.py client --server-host 100.64.0.100 --server-port 40000 --my-id B --peer-id A --direct-window 5 --timeout 10', background=True)

	codes = wait_procs({'srv': srv, 'c1': c1, 'c2': c2}, timeout=20)
	# Server exits after timeout, clients should return 0 on direct success
	assert codes['c1'] == 0, f"client1 exit {codes['c1']}"
	assert codes['c2'] == 0, f"client2 exit {codes['c2']}"


def test_udp_relay_fallback_under_symmetric_nat(nat_net_symmetric_true_block_new):
	net, nodes = nat_net_symmetric_true_block_new
	h1 = nodes['h1']
	h2 = nodes['h2']
	hinet = nodes['hinet']

	# Start rendezvous server on public internet host
	srv = run_on(hinet, 'python3 tests/udp_traversal.py server --bind-host 100.64.0.100 --bind-port 40000 --timeout 15', background=True)
	time.sleep(0.5)

	# Start both clients with relay enabled
	c1 = run_on(h1, 'python3 tests/udp_traversal.py client --server-host 100.64.0.100 --server-port 40000 --my-id A --peer-id B --direct-window 2 --relay-window 5 --enable-relay --timeout 10', background=True)
	c2 = run_on(h2, 'python3 tests/udp_traversal.py client --server-host 100.64.0.100 --server-port 40000 --my-id B --peer-id A --direct-window 2 --relay-window 5 --enable-relay --timeout 10', background=True)

	codes = wait_procs({'srv': srv, 'c1': c1, 'c2': c2}, timeout=20)
	assert codes['c1'] == 0, f"client1 exit {codes['c1']}"
	assert codes['c2'] == 0, f"client2 exit {codes['c2']}"

