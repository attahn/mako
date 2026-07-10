#!/usr/bin/env python3
"""WS client: ping→pong, then binary echo."""
import base64
import hashlib
import os
import socket
import struct
import sys


def handshake(sock, host, port):
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET / HTTP/1.1\r\nHost: {host}:{port}\r\n"
        "Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    sock.sendall(req.encode())
    data = b""
    while b"\r\n\r\n" not in data:
        data += sock.recv(4096)
    if b"101" not in data.split(b"\r\n", 1)[0]:
        raise SystemExit(f"upgrade failed: {data[:120]!r}")


def send_frame(sock, opcode, payload: bytes):
    mask = os.urandom(4)
    hdr = bytearray([0x80 | opcode])
    n = len(payload)
    if n < 126:
        hdr.append(0x80 | n)
    else:
        hdr.append(0x80 | 126)
        hdr.extend(struct.pack("!H", n))
    hdr.extend(mask)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    sock.sendall(bytes(hdr) + masked)


def recv_frame(sock):
    h = sock.recv(2)
    if len(h) < 2:
        raise SystemExit("short")
    opcode = h[0] & 0x0F
    plen = h[1] & 0x7F
    if plen == 126:
        plen = struct.unpack("!H", sock.recv(2))[0]
    data = b""
    while len(data) < plen:
        data += sock.recv(plen - len(data))
    return opcode, data


def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 18095
    sock = socket.create_connection((host, port), timeout=5)
    handshake(sock, host, port)
    send_frame(sock, 0x9, b"hi")  # ping
    op, data = recv_frame(sock)
    if op != 0xA or data != b"hi":
        raise SystemExit(f"pong fail op={op} data={data!r}")
    print("pong ok")
    send_frame(sock, 0x2, b"\x01\x02\x03")  # binary
    op, data = recv_frame(sock)
    if op != 0x2 or data != b"\x01\x02\x03":
        raise SystemExit(f"binary echo fail op={op} data={data!r}")
    print("binary ok")
    print("ws ping/binary client ok")


if __name__ == "__main__":
    main()
