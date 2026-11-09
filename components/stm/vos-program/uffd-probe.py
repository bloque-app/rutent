"""The userfaultfd write-protect probe of host-check.sh (T-010-08, G-37), standard library only.

Makes the calls VOS's write tracking makes: userfaultfd(UFFD_USER_MODE_ONLY), UFFDIO_API asking for asynchronous
write-protect, /proc/self/pagemap, then UFFDIO_REGISTER of an anonymous mapping in write-protect mode and
UFFDIO_WRITEPROTECT of one page of it.

Exit status: 0 write-protect works; 1 the host does not allow it (EPERM, EACCES, ENOSYS, EINVAL, or the feature
bit missing from the answer); 2 the probe itself failed (any other errno or any Python exception), which says
nothing about the host. UFFD_PROBE_FAKE=ok|eperm|nofeature|errno runs the same code against a fake kernel, for the
tests: the fake checks that each ioctl gets a buffer of the struct's exact size and answers in it.
"""
import ctypes
import mmap
import os
import struct
import sys
import traceback

UFFD_USER_MODE_ONLY = 1
UFFD_API = 0xAA
WP_HUGETLBFS_SHMEM = 1 << 12
WP_ASYNC = 1 << 15
SYS_USERFAULTFD = 323  # x86_64
# ioctl number, and the size of the struct it takes (_IOWR(0xAA, nr, struct)).
UFFDIO_API = (0xC018AA3F, 24)  # uffdio_api: api, features, ioctls
UFFDIO_REGISTER = (0xC020AA00, 32)  # uffdio_register: range{start, len}, mode, ioctls
UFFDIO_WRITEPROTECT = (0xC018AA06, 24)  # uffdio_writeprotect: range{start, len}, mode
MODE_WP = 2  # UFFDIO_REGISTER_MODE_WP
WRITEPROTECT_MODE_WP = 1  # UFFDIO_WRITEPROTECT_MODE_WP
NOT_ALLOWED = {1, 13, 38, 22}  # EPERM, EACCES, ENOSYS, EINVAL (Linux)


class NotAllowed(Exception):
    pass


def buffer(size, *fields):
    """A mutable buffer of exactly `size` bytes holding u64 `fields` (create_string_buffer(bytes) would add a NUL)."""
    packed = struct.pack("Q" * len(fields), *fields)
    assert len(packed) == size, (len(packed), size)
    return ctypes.create_string_buffer(packed, size)


def refused(errno, what):
    if errno in NOT_ALLOWED:
        raise NotAllowed(f"{what}: {os.strerror(errno)}")
    raise RuntimeError(f"{what}: unexpected errno {errno} ({os.strerror(errno)})")


def probe(libc, errno, close, pagemap):
    flags = os.O_CLOEXEC | os.O_NONBLOCK | UFFD_USER_MODE_ONLY
    features = WP_ASYNC | WP_HUGETLBFS_SHMEM

    def opened():
        fd = libc.syscall(SYS_USERFAULTFD, flags)
        if fd < 0:
            refused(errno(), "userfaultfd(UFFD_USER_MODE_ONLY)")
        api = buffer(UFFDIO_API[1], UFFD_API, features, 0)
        if libc.ioctl(fd, ctypes.c_ulong(UFFDIO_API[0]), api) != 0:
            refused(errno(), "UFFDIO_API with asynchronous write-protect")
        granted = struct.unpack("QQQ", api.raw[: UFFDIO_API[1]])[1]
        if granted & features != features:
            raise NotAllowed("UFFDIO_API did not grant asynchronous write-protect")
        return fd

    fd = opened()
    close(fd)
    pagemap()
    page = mmap.PAGESIZE
    area = mmap.mmap(-1, 4 * page, flags=mmap.MAP_PRIVATE | mmap.MAP_ANONYMOUS)
    start = ctypes.addressof(ctypes.c_char.from_buffer(area))
    fd = opened()
    register = buffer(UFFDIO_REGISTER[1], start, 4 * page, MODE_WP, 0)
    if libc.ioctl(fd, ctypes.c_ulong(UFFDIO_REGISTER[0]), register) != 0:
        refused(errno(), "UFFDIO_REGISTER in write-protect mode")
    protect = buffer(UFFDIO_WRITEPROTECT[1], start, page, WRITEPROTECT_MODE_WP)
    if libc.ioctl(fd, ctypes.c_ulong(UFFDIO_WRITEPROTECT[0]), protect) != 0:
        refused(errno(), "UFFDIO_WRITEPROTECT")
    close(fd)


class FakeKernel:
    """A kernel that checks every buffer's size and answers in it, or refuses as `mode` says."""

    def __init__(self, mode):
        self.mode = mode
        self.err = 0

    def syscall(self, number, *_):
        assert number == SYS_USERFAULTFD
        if self.mode == "eperm":
            self.err = 1
            return -1
        if self.mode == "errno":
            self.err = 5  # EIO: not a refusal
            return -1
        return 3

    def ioctl(self, fd, request, buf):
        sizes = {r: s for r, s in (UFFDIO_API, UFFDIO_REGISTER, UFFDIO_WRITEPROTECT)}
        assert ctypes.sizeof(buf) == sizes[request.value], "wrong buffer size for the ioctl"
        if request.value == UFFDIO_API[0]:
            api, features, _ = struct.unpack("QQQ", buf.raw)
            granted = 0 if self.mode == "nofeature" else features
            buf.raw = struct.pack("QQQ", api, granted, 0x1F)
        return 0


def main():
    fake = os.environ.get("UFFD_PROBE_FAKE")
    try:
        if fake:
            kernel = FakeKernel(fake)
            probe(kernel, lambda: kernel.err, lambda fd: None, lambda: None)
        else:
            libc = ctypes.CDLL(None, use_errno=True)
            probe(
                libc,
                ctypes.get_errno,
                os.close,
                lambda: os.close(os.open("/proc/self/pagemap", os.O_RDONLY | os.O_CLOEXEC)),
            )
    except NotAllowed as why:
        print(f"uffd-probe: not allowed: {why}", file=sys.stderr)
        return 1
    except BaseException:
        print("uffd-probe: SCRIPT ERROR, the probe itself failed:", file=sys.stderr)
        traceback.print_exc()
        return 2
    return 0


sys.exit(main())
