#!/usr/bin/env python3
"""A stand-in iPhone for GVfs's AFC backend, on a usbmuxd socket of its own
(tools/test/gvfs-afc.sh, ADR-078).

usbmuxd's plist protocol (ListDevices, Listen, ReadBUID, ReadPairRecord,
Connect), lockdownd's (QueryType, GetValue, StartSession without TLS,
StartService for com.apple.afc) and AFC's packets over a folder. Every AFC
request is logged, one line each: its operation, the bytes it carried and
the bytes answered, so the requests a way of copying makes can be counted.
Its pace says nothing of an iPhone's: an iPhone serves AFC one request at a
time over USB, each a round trip.

    afc-standin.py <socket> <root folder> <log>
"""
import errno
import os
import plistlib
import shutil
import socket
import socketserver
import struct
import sys
import threading
import time

SOCK, ROOT, LOG = sys.argv[1:4]
UDID = "0123456789abcdef0123456789abcdef01234567"
BUID = "00000000-0000-0000-0000-000000000001"
HOST_ID = "00000000-0000-0000-0000-000000000002"
LOCKDOWN = 62078
AFC = 52001
VALUES = {
    "ProductVersion": "17.0",
    "DeviceClass": "iPhone",
    "DeviceName": "Test phone",
    "UniqueDeviceID": UDID,
    "ProductType": "iPhone14,2",
    "BuildVersion": "21A329",
}
OPS = {
    1: "STATUS", 2: "DATA", 3: "READ_DIR", 7: "TRUNCATE", 8: "REMOVE_PATH",
    9: "MAKE_DIR", 10: "GET_FILE_INFO", 11: "GET_DEVINFO", 13: "FILE_OPEN",
    14: "FILE_OPEN_RES", 15: "FILE_READ", 16: "FILE_WRITE", 17: "FILE_SEEK",
    18: "FILE_TELL", 19: "FILE_TELL_RES", 20: "FILE_CLOSE", 21: "FILE_SET_SIZE",
    22: "GET_CON_INFO", 23: "SET_CON_OPTIONS", 24: "RENAME_PATH",
    25: "SET_FS_BS", 26: "SET_SOCKET_BS", 27: "FILE_LOCK", 30: "SET_FILE_MOD_TIME",
    34: "REMOVE_PATH_AND_CONTENTS",
}
ERRNO = {
    errno.ENOENT: 8, errno.EISDIR: 9, errno.EACCES: 10, errno.EPERM: 10,
    errno.EEXIST: 16, errno.ENOTEMPTY: 33, errno.ENOSPC: 18,
}
log_lock = threading.Lock()
log = open(LOG, "a", buffering=1)


def note(line):
    with log_lock:
        log.write(f"{time.monotonic():.6f} {line}\n")


def recv_exact(sock, n):
    data = bytearray()
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            raise EOFError
        data += chunk
    return bytes(data)


def device():
    return {
        "DeviceID": 1,
        "MessageType": "Attached",
        "Properties": {
            "ConnectionSpeed": 480000000,
            "ConnectionType": "USB",
            "DeviceID": 1,
            "LocationID": 1,
            "ProductID": 0x12A8,
            "SerialNumber": UDID,
            "UDID": UDID,
            "USBSerialNumber": UDID,
        },
    }


class Muxd(socketserver.BaseRequestHandler):
    def send(self, tag, message):
        payload = plistlib.dumps(message)
        self.request.sendall(struct.pack("<IIII", 16 + len(payload), 1, 8, tag) + payload)

    def handle(self):
        sock = self.request
        try:
            while True:
                length, _version, _kind, tag = struct.unpack("<IIII", recv_exact(sock, 16))
                message = plistlib.loads(recv_exact(sock, length - 16))
                kind = message.get("MessageType")
                if kind == "ListDevices":
                    self.send(tag, {"DeviceList": [device()]})
                elif kind == "Listen":
                    self.send(tag, {"MessageType": "Result", "Number": 0})
                    self.send(0, device())
                elif kind == "ReadBUID":
                    self.send(tag, {"BUID": BUID})
                elif kind == "ReadPairRecord":
                    record = {"HostID": HOST_ID, "SystemBUID": BUID,
                              "WiFiMACAddress": "00:00:00:00:00:00"}
                    self.send(tag, {"PairRecordData": plistlib.dumps(record)})
                elif kind == "Connect":
                    port = socket.ntohs(message["PortNumber"])
                    self.send(tag, {"MessageType": "Result", "Number": 0})
                    if port == LOCKDOWN:
                        Lockdown(sock).run()
                    elif port == AFC:
                        Afc(sock).run()
                    return
                else:
                    self.send(tag, {"MessageType": "Result", "Number": 0})
        except (EOFError, ConnectionError):
            return


class Lockdown:
    def __init__(self, sock):
        self.sock = sock

    def reply(self, message):
        payload = plistlib.dumps(message)
        self.sock.sendall(struct.pack(">I", len(payload)) + payload)

    def run(self):
        try:
            while True:
                (length,) = struct.unpack(">I", recv_exact(self.sock, 4))
                message = plistlib.loads(recv_exact(self.sock, length))
                request = message.get("Request")
                answer = {"Request": request, "Result": "Success"}
                if request == "QueryType":
                    answer["Type"] = "com.apple.mobile.lockdown"
                elif request == "GetValue":
                    key = message.get("Key")
                    if key is None:
                        answer["Value"] = VALUES
                    elif key in VALUES:
                        answer["Key"], answer["Value"] = key, VALUES[key]
                    else:
                        answer = {"Request": request, "Error": "MissingValue"}
                elif request == "StartSession":
                    answer["SessionID"] = "test-session"
                    answer["EnableSessionSSL"] = False
                elif request == "StartService":
                    service = message.get("Service")
                    if service == "com.apple.afc":
                        answer.update(Service=service, Port=AFC, EnableServiceSSL=False)
                    else:
                        answer = {"Request": request, "Error": "InvalidService"}
                elif request not in ("StopSession", "Goodbye"):
                    note(f"lockdown unanswered {request}")
                    answer = {"Request": request, "Error": "UnknownRequest"}
                self.reply(answer)
                if request == "Goodbye":
                    return
        except (EOFError, ConnectionError):
            return


class Afc:
    def __init__(self, sock):
        self.sock = sock
        self.handles = {}
        self.next = 1

    def path(self, raw):
        name = raw.split(b"\0", 1)[0].decode()
        full = os.path.normpath(os.path.join(ROOT, name.lstrip("/")))
        if full != ROOT and not full.startswith(ROOT + os.sep):
            raise OSError(errno.EACCES, "outside")
        return full

    def reply(self, number, op, data=b""):
        # The header alone first, what it carries after (as a device does):
        # libimobiledevice reads the first part in one receive.
        header = struct.pack("<8sQQQQ", b"CFA6LPAA", 40 + len(data), 40, number, op)
        self.sock.sendall(header + data)
        return len(data)

    def status(self, number, code=0):
        return self.reply(number, 1, struct.pack("<Q", code))

    def info(self, full):
        st = os.lstat(full)
        kind = "S_IFDIR" if os.path.isdir(full) and not os.path.islink(full) else (
            "S_IFLNK" if os.path.islink(full) else "S_IFREG")
        pairs = [("st_size", st.st_size), ("st_blocks", st.st_blocks), ("st_nlink", st.st_nlink),
                 ("st_ifmt", kind), ("st_mtime", st.st_mtime_ns), ("st_birthtime", st.st_mtime_ns)]
        return b"".join(f"{k}\0{v}\0".encode() for k, v in pairs)

    def serve(self, number, op, data, payload):
        if op == 3:
            names = [".", ".."] + sorted(os.listdir(self.path(data)))
            return self.reply(number, 2, b"".join(n.encode() + b"\0" for n in names))
        if op == 10:
            return self.reply(number, 2, self.info(self.path(data)))
        if op == 11:
            usage = shutil.disk_usage(ROOT)
            pairs = [("Model", "iPhone14,2"), ("FSTotalBytes", usage.total),
                     ("FSFreeBytes", usage.free), ("FSBlockSize", 4096)]
            return self.reply(number, 2, b"".join(f"{k}\0{v}\0".encode() for k, v in pairs))
        if op == 13:
            (mode,) = struct.unpack("<Q", data[:8])
            flags = {1: os.O_RDONLY, 2: os.O_RDWR | os.O_CREAT,
                     3: os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 4: os.O_RDWR | os.O_CREAT | os.O_TRUNC,
                     5: os.O_WRONLY | os.O_APPEND | os.O_CREAT,
                     6: os.O_RDWR | os.O_APPEND | os.O_CREAT}[mode]
            fd = os.open(self.path(data[8:]), flags, 0o644)
            handle, self.next = self.next, self.next + 1
            self.handles[handle] = fd
            return self.reply(number, 14, struct.pack("<Q", handle))
        if op == 15:
            handle, size = struct.unpack("<QQ", data[:16])
            return self.reply(number, 2, os.read(self.handles[handle], size))
        if op == 16:
            (handle,) = struct.unpack("<Q", data[:8])
            os.write(self.handles[handle], payload)
            return self.status(number)
        if op == 17:
            handle, whence, offset = struct.unpack("<QQq", data[:24])
            os.lseek(self.handles[handle], offset, whence)
            return self.status(number)
        if op == 18:
            (handle,) = struct.unpack("<Q", data[:8])
            return self.reply(number, 19, struct.pack("<Q", os.lseek(self.handles[handle], 0, 1)))
        if op == 20:
            (handle,) = struct.unpack("<Q", data[:8])
            os.close(self.handles.pop(handle))
            return self.status(number)
        if op == 21:
            handle, size = struct.unpack("<QQ", data[:16])
            os.ftruncate(self.handles[handle], size)
            return self.status(number)
        if op == 7:
            (size,) = struct.unpack("<Q", data[:8])
            os.truncate(self.path(data[8:]), size)
            return self.status(number)
        if op == 9:
            full = self.path(data)
            if not os.path.isdir(full):
                os.mkdir(full)
            return self.status(number)
        if op == 8:
            full = self.path(data)
            if os.path.isdir(full) and not os.path.islink(full):
                os.rmdir(full)
            else:
                os.unlink(full)
            return self.status(number)
        if op == 34:
            full = self.path(data)
            if os.path.isdir(full) and not os.path.islink(full):
                shutil.rmtree(full)
            else:
                os.unlink(full)
            return self.status(number)
        if op == 24:
            source, target = data.split(b"\0")[:2]
            os.rename(self.path(source), self.path(target))
            return self.status(number)
        if op == 30:
            (ns,) = struct.unpack("<Q", data[:8])
            os.utime(self.path(data[8:]), ns=(ns, ns))
            return self.status(number)
        if op in (22, 23, 25, 26, 27):
            return self.status(number)
        return self.status(number, 15)

    def run(self):
        try:
            while True:
                magic, entire, this, number, op = struct.unpack("<8sQQQQ", recv_exact(self.sock, 40))
                if magic != b"CFA6LPAA":
                    return
                data = recv_exact(self.sock, this - 40)
                payload = recv_exact(self.sock, entire - this)
                try:
                    answered = self.serve(number, op, data, payload)
                except OSError as error:
                    answered = self.status(number, ERRNO.get(error.errno, 1))
                except (KeyError, ValueError, struct.error):
                    answered = self.status(number, 7)
                note(f"afc {OPS.get(op, op)} {len(data) + len(payload)} {answered}")
        except (EOFError, ConnectionError):
            for fd in self.handles.values():
                os.close(fd)


class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


if os.path.exists(SOCK):
    os.unlink(SOCK)
with Server(SOCK, Muxd) as server:
    note("ready")
    server.serve_forever()
