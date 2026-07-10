#!/usr/bin/env python3
"""Two HTTP/1.1 GETs on one keep-alive socket."""
import socket
import sys

host = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
port = int(sys.argv[2]) if len(sys.argv) > 2 else 18094

s = socket.create_connection((host, port), timeout=5)
req1 = (
    f"GET /one HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: keep-alive\r\n\r\n"
).encode()
req2 = (
    f"GET /two HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
).encode()
s.sendall(req1)


def read_response(sock):
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(4096)
        if not chunk:
            break
        data += chunk
    if b"\r\n\r\n" not in data:
        raise SystemExit(f"no headers: {data!r}")
    head, rest = data.split(b"\r\n\r\n", 1)
    cl = 0
    for line in head.split(b"\r\n")[1:]:
        if line.lower().startswith(b"content-length:"):
            cl = int(line.split(b":", 1)[1].strip())
    while len(rest) < cl:
        chunk = sock.recv(4096)
        if not chunk:
            break
        rest += chunk
    body = rest[:cl]
    leftover = rest[cl:]
    return head.decode(errors="replace"), body.decode(errors="replace"), leftover


h1, b1, left = read_response(s)
if left:
    # shouldn't happen with exact CL
    pass
print("R1", b1.strip())
if "keep-alive" not in h1.lower() and "Keep-Alive" not in h1:
    # server should advertise keep-alive on first response
    if "connection: keep-alive" not in h1.lower():
        print("WARN: first response missing keep-alive header")
        print(h1)

s.sendall(req2)
h2, b2, _ = read_response(s)
print("R2", b2.strip())
if b1.strip() != "/one" or b2.strip() != "/two":
    raise SystemExit(f"body mismatch: {b1!r} {b2!r}")
print("keepalive client ok")
s.close()
