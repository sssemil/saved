#!/usr/bin/env python3
import os
import time

import pytest

from mininet_utils import build_lan_topology, run_on, wait_procs


@pytest.fixture(scope="function")
def lan_net():
	net, nodes = build_lan_topology()
	try:
		yield net, nodes
	finally:
		# Clean up
		net.stop()


def test_ping_between_hosts(lan_net):
	net, nodes = lan_net
	h1 = nodes['h1']
	h2 = nodes['h2']

	# Quick reachability
	res = run_on(h1, 'ping -c 2 -W 1 10.0.0.2')
	assert ', 0% packet loss' in res, res


def test_udp_tcp_basic(lan_net):
	net, nodes = lan_net
	h1 = nodes['h1']
	h2 = nodes['h2']

	# Start iperf3 servers on h2
	udp_srv = run_on(h2, 'iperf3 -s -1 -p 5002 -B 10.0.0.2 -u', background=True)
	tcp_srv = run_on(h2, 'iperf3 -s -1 -p 5001 -B 10.0.0.2', background=True)
	# Give servers a moment
	time.sleep(0.5)

	# UDP client from h1
	udp_out = run_on(h1, 'iperf3 -c 10.0.0.2 -u -b 5M -t 1 -p 5002 -R')
	assert 'receiver' in udp_out.lower() or 'sender' in udp_out.lower(), udp_out

	# TCP client from h1
	tcp_out = run_on(h1, 'iperf3 -c 10.0.0.2 -t 1 -p 5001 -R')
	assert 'receiver' in tcp_out.lower() or 'sender' in tcp_out.lower(), tcp_out

	# Ensure servers exited
	codes = wait_procs({'udp': udp_srv, 'tcp': tcp_srv}, timeout=10)
	assert codes['udp'] == 0
	assert codes['tcp'] == 0

