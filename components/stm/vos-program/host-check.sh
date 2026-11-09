#!/bin/sh
# The production-host check (`T-010-08`, `G-37`). Fails unless the host can run VOS's JIT as the program needs it:
#   1. Linux x86_64 on kernel 6.7 or later (the kernel that gives userfaultfd asynchronous write-protect);
#   2. userfaultfd write-protect allowed to an unprivileged process (not blocked by sysctl, seccomp or a container);
#   3. the JIT's perf map appears: a node built by build.sh, started on the JIT, runs the program and names it in
#      /tmp/perf-<pid>.map (the client's test `a_vos_node_on_the_jit_names_the_program_in_its_perf_map`).
# Every fact is checked and reported, so one run shows all that is wrong. CI runs it on the runner after the build
# (`.github/workflows/stm-vos.yml`), and `F-030` runs it on the production host.
# Usage: VOS_DIR=<checkout from fetch-vos.sh> host-check.sh BUILD_DIR     (BUILD_DIR is what build.sh made)
# Needs: sh, uname, and python3 for the userfaultfd probe (the standard library only).
# Hooks, for the tests that stand in for a host that lacks a fact (client/tests/host_check.rs): HOST_CHECK_OS,
# HOST_CHECK_KERNEL, and HOST_CHECK_UFFD and HOST_CHECK_PERF, each a shell command whose exit status is the answer.
# Left unset they read the real host. Never set them on a host being checked.
set -u
here=$(cd "$(dirname "$0")" && pwd -P)
failed=0
fail() {
    echo "host-check: FAIL: $*" >&2
    failed=1
}
pass() {
    echo "host-check: ok: $*"
}

# 1. The platform and the kernel.
os=${HOST_CHECK_OS:-$(uname -sm)}
kernel=${HOST_CHECK_KERNEL:-$(uname -r)}
if [ "$os" != "Linux x86_64" ]; then
    fail "the kernel check wants Linux x86_64, not $os"
else
    major=$(printf '%s' "$kernel" | sed -n 's/^\([0-9][0-9]*\).*/\1/p')
    minor=$(printf '%s' "$kernel" | sed -n 's/^[0-9][0-9]*\.\([0-9][0-9]*\).*/\1/p')
    if [ -z "$major" ]; then
        fail "the kernel version \"$kernel\" does not read as MAJOR.MINOR"
    elif [ "$major" -eq 6 ] && [ "${minor:-0}" -eq 7 ] && printf '%s' "$kernel" | grep -q -- '-rc'; then
        # 6.7's release candidates predate the release that has asynchronous write-protect settled.
        fail "the kernel is $kernel, a release candidate of 6.7, and the JIT needs 6.7 or later"
    elif [ "$major" -gt 6 ] || { [ "$major" -eq 6 ] && [ "${minor:-0}" -ge 7 ]; }; then
        pass "the kernel is $kernel (6.7 or later)"
    else
        fail "the kernel is $kernel, and the JIT needs 6.7 or later"
    fi
fi

# 2. userfaultfd with asynchronous write-protect, probed as VOS's write tracking asks for it, then used: a page of an
# anonymous mapping is registered in WP mode and write-protected.
uffd_probe() {
    python3 - <<'PY'
import ctypes, os, struct, sys
UFFD_USER_MODE_ONLY = 1
UFFD_API = 0xAA
WP_HUGETLBFS_SHMEM = 1 << 12
WP_ASYNC = 1 << 15
UFFDIO_API = 0xC018AA3F
SYS_USERFAULTFD = 323  # x86_64
libc = ctypes.CDLL(None, use_errno=True)
fd = libc.syscall(SYS_USERFAULTFD, os.O_CLOEXEC | os.O_NONBLOCK | UFFD_USER_MODE_ONLY)
if fd < 0:
    sys.exit("userfaultfd(UFFD_USER_MODE_ONLY): " + os.strerror(ctypes.get_errno()))
features = WP_ASYNC | WP_HUGETLBFS_SHMEM
api = ctypes.create_string_buffer(struct.pack("QQQ", UFFD_API, features, 0))
rc = libc.ioctl(fd, ctypes.c_ulong(UFFDIO_API), api)
err = os.strerror(ctypes.get_errno())
os.close(fd)
if rc != 0:
    sys.exit("UFFDIO_API with asynchronous write-protect: " + err)
if struct.unpack("QQQ", api.raw)[1] & features != features:
    sys.exit("UFFDIO_API did not grant asynchronous write-protect")
os.close(os.open("/proc/self/pagemap", os.O_RDONLY | os.O_CLOEXEC))
# Write-protect itself: register an anonymous mapping in WP mode and write-protect one page of it. A host whose
# UFFDIO_API answers but whose registration or protection is refused (a seccomp filter on the ioctl, say) fails here.
# The real test remains step 3, a node on the JIT.
import mmap
UFFDIO_REGISTER = 0xC020AA00      # _IOWR(0xAA, 0x00, struct uffdio_register: range, mode, ioctls = 32 bytes)
UFFDIO_WRITEPROTECT = 0xC018AA06  # _IOWR(0xAA, 0x06, struct uffdio_writeprotect: range, mode = 24 bytes)
MODE_WP = 2                       # UFFDIO_REGISTER_MODE_WP
WRITEPROTECT_MODE_WP = 1          # UFFDIO_WRITEPROTECT_MODE_WP
page = mmap.PAGESIZE
area = mmap.mmap(-1, 4 * page, flags=mmap.MAP_PRIVATE | mmap.MAP_ANONYMOUS)
start = ctypes.addressof(ctypes.c_char.from_buffer(area))
fd = libc.syscall(SYS_USERFAULTFD, os.O_CLOEXEC | os.O_NONBLOCK | UFFD_USER_MODE_ONLY)
if fd < 0:
    sys.exit("userfaultfd(UFFD_USER_MODE_ONLY): " + os.strerror(ctypes.get_errno()))
api = ctypes.create_string_buffer(struct.pack("QQQ", UFFD_API, features, 0))
if libc.ioctl(fd, ctypes.c_ulong(UFFDIO_API), api) != 0:
    sys.exit("UFFDIO_API: " + os.strerror(ctypes.get_errno()))
register = ctypes.create_string_buffer(struct.pack("QQQQ", start, 4 * page, MODE_WP, 0))
if libc.ioctl(fd, ctypes.c_ulong(UFFDIO_REGISTER), register) != 0:
    sys.exit("UFFDIO_REGISTER in write-protect mode: " + os.strerror(ctypes.get_errno()))
protect = ctypes.create_string_buffer(struct.pack("QQQ", start, page, WRITEPROTECT_MODE_WP))
if libc.ioctl(fd, ctypes.c_ulong(UFFDIO_WRITEPROTECT), protect) != 0:
    sys.exit("UFFDIO_WRITEPROTECT: " + os.strerror(ctypes.get_errno()))
os.close(fd)
PY
}
if [ -n "${HOST_CHECK_UFFD:-}" ]; then
    sh -c "$HOST_CHECK_UFFD"
else
    uffd_probe
fi
# shellcheck disable=SC2181
if [ $? -eq 0 ]; then
    pass "userfaultfd write-protect is allowed"
else
    fail "userfaultfd write-protect is not allowed to this process"
fi

# 3. The JIT's perf map appears.
if [ -n "${HOST_CHECK_PERF:-}" ]; then
    sh -c "$HOST_CHECK_PERF"
elif [ $# -ge 1 ] && [ -n "${VOS_DIR:-}" ]; then
    "$here/test.sh" "$1" --test vos_node a_vos_node_on_the_jit_names_the_program_in_its_perf_map
else
    echo "host-check: no build directory and no VOS_DIR, so no node can run" >&2
    false
fi
# shellcheck disable=SC2181
if [ $? -eq 0 ]; then
    pass "the JIT's perf map names the program"
else
    fail "the JIT's perf map did not appear"
fi

if [ "$failed" -eq 0 ]; then
    echo "host-check: this host can run the JIT"
else
    echo "host-check: this host cannot run the JIT" >&2
fi
exit "$failed"
