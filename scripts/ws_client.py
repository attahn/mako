#!/usr/bin/env python3
"""Minimal RFC6455 client — no third-party deps. Echo smoke for mako ws_echo_once."""
import base64
import hashlib
import os
import socket
import struct
import sys


def ws_key():
    return base64.b64encode(os.urandom(16)).decode()


def handshake(sock, host, port, path="/"):
    key = ws_key()
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "\r\n"
    )
    sock.sendall(req.encode())
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(4096)
        if not chunk:
            break
        data += chunk
    if b"101" not in data.split(b"\r\n", 1)[0]:
        raise SystemExit(f"upgrade failed: {data[:200]!r}")
    expect = base64.b64encode(
        hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
    ).decode()
    if expect.encode() not in data:
        raise SystemExit("bad Sec-WebSocket-Accept")
    return True


def send_text(sock, text: str):
    payload = text.encode()
    mask = os.urandom(4)
    hdr = bytearray([0x81])
    n = len(payload)
    if n < 126:
        hdr.append(0x80 | n)
    else:
        hdr.append(0x80 | 126)
        hdr.extend(struct.pack("!H", n))
    hdr.extend(mask)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    sock.sendall(bytes(hdr) + masked)


def recv_text(sock) -> str:
    h = sock.recv(2)
    if len(h) < 2:
        raise SystemExit("short header")
    plen = h[1] & 0x7F
    if plen == 126:
        plen = struct.unpack("!H", sock.recv(2))[0]
    data = b""
    while len(data) < plen:
        data += sock.recv(plen - len(data))
    return data.decode()


def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 18092
    msg = sys.argv[3] if len(sys.argv) > 3 else "ping-mako"
    sock = socket.create_connection((host, port), timeout=5)
    handshake(sock, host, port)
    send_text(sock, msg)
    out = recv_text(sock)
    print(out)
    if out != msg:
        raise SystemExit(f"echo mismatch: {out!r}")
    print("ws client ok")


if __name__ == "__main__":
    main()
