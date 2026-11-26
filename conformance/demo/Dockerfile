# syntax=docker/dockerfile:1.7
# The demo API's image (conformance/demo/README.md, "In a container"): rutent-demo-api serving the synthetic network,
# its page and its transitions database's API. Nothing in it holds or moves real value.
#
# linux/amd64 only, and it needs a runtime that lets it make namespaces. The platform runs every provider's module
# in a jail of its own (rutent_prov::module::sandbox, DEC-27): unshare of user, network, IPC, UTS, mount and PID
# namespaces, pivot_root, Landlock and a seccomp allow-list written for x86_64, which refuses to run unconfined on
# any other architecture. Docker's default seccomp profile refuses unshare, so run it with a seccomp profile that
# allows it (seccomp=unconfined works) and Docker's default capabilities, on a kernel that allows unprivileged user
# namespaces. Fargate allows neither; ECS on EC2 (x86_64) does.
#
# Build from the repository's root:
#   docker build -f conformance/demo/Dockerfile -t rutent-demo-api .
#
# One stage on purpose: the assembly finds the platform's executables and the simulated providers' modules beside a
# build directory and checks them against the sources (rutent_scenarios::sim::disk), so the image keeps the sources,
# the toolchain and the build. It is large; it is a test image.
FROM rust:1.98-bookworm

WORKDIR /rutent
COPY . .

# The toolchain the repository pins (rust-toolchain.toml), with the wasm32 target the providers' modules build for.
RUN rustup show active-toolchain || rustup toolchain install

ENV CARGO_TARGET_DIR=/rutent/target
# The demo API is a workspace of its own (conformance/scenarios/demo-api); audit-check is the root workspace's.
RUN cargo build --locked --manifest-path conformance/scenarios/demo-api/Cargo.toml \
    && cargo build --locked -p rutent-core-audit --bin audit-check

# Builds the platform's executables, the providers' and the wallet's modules and audit-check, so a container
# answers within about a minute of starting (Core admitting the modules) instead of building first. It doesn't start
# Core: Core starts the platform's jailed processes, which the build's default sandbox refuses.
RUN /rutent/target/debug/rutent-demo-api --warm-up

# The certificate authorities of Amazon RDS, for the database's TLS (DATABASE_CA_FILE). Fetched when the image is
# built, from AWS's trust store.
ADD https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem /etc/rutent/rds-ca.pem

ENV DATABASE_CA_FILE=/etc/rutent/rds-ca.pem
EXPOSE 7040
HEALTHCHECK --interval=30s --timeout=5s --start-period=180s --retries=3 \
    CMD curl -fsS http://127.0.0.1:7040/health || exit 1
ENTRYPOINT ["/rutent/target/debug/rutent-demo-api", "--addr", "0.0.0.0:7040"]
