#!/usr/bin/env python3
"""
UDP traversal demo used by tests:
- server: rendezvous (shares observed endpoints) and simple relay
- client: tries direct UDP hole punching, then optional relay fallback

This is intentionally tiny and not a full ICE/STUN/TURN implementation.
"""

from __future__ import annotations

import argparse
import base64
import os
import socket
import sys
import time
from typing import Dict, Optional, Tuple


def b64(data: bytes) -> str:
	return base64.b64encode(data).decode('ascii')


def b64d(s: str) -> bytes:
	return base64.b64decode(s.encode('ascii'))


class RendezvousServer:
	def __init__(self, bind_host: str, bind_port: int):
		self.bind_host = bind_host
		self.bind_port = bind_port
		self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
		self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
		self.sock.bind((self.bind_host, self.bind_port))
		self.peers: Dict[str, Tuple[str, int]] = {}
		self.pairs: Dict[str, str] = {}

	def serve_forever(self, timeout: float = 60.0) -> None:
		self.sock.settimeout(0.5)
		end = time.time() + timeout
		while time.time() < end:
			try:
				data, addr = self.sock.recvfrom(4096)
			except socket.timeout:
				continue
			except Exception:
				break
			self.handle_message(data.decode('utf-8', errors='replace').strip(), addr)

	def _send(self, addr: Tuple[str, int], msg: str) -> None:
		self.sock.sendto(msg.encode('utf-8'), addr)

	def handle_message(self, msg: str, addr: Tuple[str, int]) -> None:
		parts = msg.split()
		if not parts:
			return
		cmd = parts[0].upper()
		if cmd == 'PAIR' and len(parts) == 3:
			my_id, peer_id = parts[1], parts[2]
			self.peers[my_id] = addr
			self.pairs[my_id] = peer_id
			# If peer already known, send each other's endpoints
			if peer_id in self.peers:
				paddr = self.peers[peer_id]
				self._send(addr, f'PEER {paddr[0]} {paddr[1]} {peer_id}')
				self._send(paddr, f'PEER {addr[0]} {addr[1]} {my_id}')
			else:
				self._send(addr, 'OK WAIT')
		elif cmd == 'ALIVE' and len(parts) == 2:
			my_id = parts[1]
			self.peers[my_id] = addr
		elif cmd == 'RELAY' and len(parts) >= 3:
			peer_id = parts[1]
			payload = ' '.join(parts[2:])
			if peer_id in self.peers:
				paddr = self.peers[peer_id]
				self._send(paddr, f'RELAY_FROM {addr[0]} {addr[1]} {payload}')
		else:
			# Unknown; echo for debugging
			self._send(addr, 'ERR')


def run_server(args) -> int:
	server = RendezvousServer(args.bind_host, args.bind_port)
	server.serve_forever(timeout=args.timeout)
	return 0


def run_client(args) -> int:
	server_addr = (args.server_host, args.server_port)
	my_id = args.my_id
	peer_id = args.peer_id

	# Single UDP socket for both server and direct traffic
	sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
	sock.bind(('0.0.0.0', 0))
	sock.settimeout(0.2)

	def send(msg: str, addr: Tuple[str, int]):
		return sock.sendto(msg.encode('utf-8'), addr)

	def recv() -> Optional[Tuple[str, Tuple[str, int]]]:
		try:
			data, addr = sock.recvfrom(4096)
			return data.decode('utf-8', errors='replace'), addr
		except socket.timeout:
			return None

	# Register pair
	send(f'PAIR {my_id} {peer_id}', server_addr)

	peer_ep: Optional[Tuple[str, int]] = None
	start = time.time()
	while time.time() - start < args.timeout:
		m = recv()
		if not m:
			# Keep alive
			send(f'ALIVE {my_id}', server_addr)
			continue
		msg, addr = m
		parts = msg.split()
		if parts and parts[0] == 'PEER' and len(parts) >= 4 and parts[3] == peer_id:
			peer_ep = (parts[1], int(parts[2]))
			break

	if peer_ep is None:
		return 2

	# Try direct punching
	ascii_payload = f'DIRECT_HELLO_{my_id}'
	deadline = time.time() + args.direct_window
	got_direct = False
	while time.time() < deadline:
		send(ascii_payload, peer_ep)
		m = recv()
		if not m:
			continue
		msg, addr = m
		if ascii_payload in msg or f'DIRECT_HELLO_{peer_id}' in msg:
			got_direct = True
			break

	if got_direct:
		return 0

	if not args.enable_relay:
		return 3

	# Relay fallback: both sides already send ALIVE to server, so reverse path is open
	relay_payload = b64(f'RELAY_HELLO_{my_id}'.encode('utf-8'))
	deadline = time.time() + args.relay_window
	while time.time() < deadline:
		send(f'RELAY {peer_id} {relay_payload}', server_addr)
		m = recv()
		if not m:
			continue
		msg, addr = m
		parts = msg.split()
		if parts and parts[0] == 'RELAY_FROM' and parts[-1] == relay_payload:
			return 0

	return 4


def main() -> int:
	parser = argparse.ArgumentParser()
	sub = parser.add_subparsers(dest='mode', required=True)

	ps = sub.add_parser('server')
	ps.add_argument('--bind-host', default='0.0.0.0')
	ps.add_argument('--bind-port', type=int, default=40000)
	ps.add_argument('--timeout', type=float, default=60.0)
	ps.set_defaults(func=run_server)

	pc = sub.add_parser('client')
	pc.add_argument('--server-host', required=True)
	pc.add_argument('--server-port', type=int, default=40000)
	pc.add_argument('--my-id', required=True)
	pc.add_argument('--peer-id', required=True)
	pc.add_argument('--direct-window', type=float, default=3.0)
	pc.add_argument('--relay-window', type=float, default=5.0)
	pc.add_argument('--timeout', type=float, default=10.0)
	pc.add_argument('--enable-relay', action='store_true')
	pc.set_defaults(func=run_client)

	args = parser.parse_args()
	return args.func(args)


if __name__ == '__main__':
	sys.exit(main())

