#!/bin/bash
# Runs the download-integrity scenarios in a throwaway Ubuntu 22.04 VM (kernel
# 5.15, apt 2.4, as on the Jetson L4T 36 devices). A power cut needs a loop
# ext4 mount and EXT4_IOC_SHUTDOWN, and the dpkg scenarios change the package
# database, so this cannot run on a CI runner or on a developer's own system.
#
# Needs rootless podman. KVM is used when /dev/kvm is usable, else qemu TCG.
#
#   run-vm.sh <workdir> <old-ref> [harness args...]
#
# Builds smithd and smith-updater from HEAD ("head") and from <old-ref>
# ("main") for Ubuntu 22.04, boots the VM, copies everything in and runs
# harness.py with the given args, for example:
#   run-vm.sh /var/tmp/e2e origin/main powercut --cut-with main --resume-with main --trials 20
# With no harness args it runs the full set. Results: <workdir>/results.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
work=$(mkdir -p "$1" && cd "$1" && pwd)
old_ref=$2
shift 2
port=${E2E_SSH_PORT:-22622}
mkdir -p "$work"/{vm,src-head,src-main,target-head,target-main,registry,bin,cdn,results}

podman build -q -t smith-e2e-qemu - > /dev/null <<'EOF'
FROM docker.io/library/ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends qemu-system-x86 qemu-utils cloud-image-utils && rm -rf /var/lib/apt/lists/*
EOF
podman build -q -t smith-e2e-build - > /dev/null <<'EOF'
FROM docker.io/library/ubuntu:22.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends build-essential pkg-config perl make curl ca-certificates && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.91.0
EOF

# Binaries for glibc 2.35. Dependencies are optimized so MD5 and TLS are not
# the bottleneck under TCG.
build() { # name ref
    rm -rf "${work:?}/src-$1" && mkdir -p "$work/src-$1"
    git -C "$repo" archive "$2" | tar -x -C "$work/src-$1"
    podman run --rm -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" -e CARGO_TARGET_DIR=/target \
        -v "$work/src-$1:/src:Z" -v "$work/target-$1:/target:Z" -v "$work/registry:/opt/cargo/registry:Z" \
        smith-e2e-build sh -c "cd /src && cargo build --locked -p smith -p smith-updater \
            --config 'profile.dev.package.\"*\".opt-level=2' --config 'profile.dev.opt-level=1'"
    cp "$work/target-$1/debug/smithd" "$work/bin/smithd-$1"
    cp "$work/target-$1/debug/smith-updater" "$work/bin/smith-updater-$1"
}
build head HEAD
build main "$old_ref"
podman run --rm -v "$here:/h:ro" -v "$work/cdn:/out:Z" smith-e2e-build /h/make_debs.sh /out

cd "$work/vm"
[ -f jammy.img ] || curl -sSfL -o jammy.img https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-amd64.img
[ -f id_e2e ] || ssh-keygen -q -t ed25519 -N '' -f id_e2e
printf '#cloud-config\nusers:\n  - name: e2e\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    shell: /bin/bash\n    ssh_authorized_keys:\n      - %s\n' \
    "$(cat id_e2e.pub)" > user-data
printf 'instance-id: smith-e2e\nlocal-hostname: smith-e2e\n' > meta-data
rm -f disk.qcow2
podman run --rm -v "$PWD:/vm:Z" smith-e2e-qemu sh -c \
    'cd /vm && cloud-localds seed.img user-data meta-data && qemu-img create -q -f qcow2 -F qcow2 -b jammy.img disk.qcow2 24G'
accel=tcg,thread=multi kvm=()
if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then accel=kvm kvm=(--device /dev/kvm); fi
podman rm -f smith-e2e-vm > /dev/null 2>&1 || true
podman run -d --name smith-e2e-vm "${kvm[@]}" -p "127.0.0.1:$port:$port" -v "$PWD:/vm:Z" smith-e2e-qemu \
    qemu-system-x86_64 -machine q35 -accel "$accel" -cpu max -smp 4 -m 4096 -nographic \
    -serial file:/vm/serial.log -drive file=/vm/disk.qcow2,if=virtio,cache=unsafe \
    -drive file=/vm/seed.img,if=virtio,format=raw \
    -netdev "user,id=n0,hostfwd=tcp::$port-:22" -device virtio-net-pci,netdev=n0 > /dev/null
trap 'podman rm -f smith-e2e-vm > /dev/null' EXIT

ssh_vm() { ssh -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 \
    -i "$work/vm/id_e2e" -p "$port" e2e@127.0.0.1 "$@"; }
until ssh_vm true 2> /dev/null; do sleep 10; done
ssh_vm 'sudo cloud-init status --wait > /dev/null; sudo mkdir -p /opt/e2e/shim /opt/e2e/state && sudo chown -R e2e /opt/e2e'
tar -C "$work" -c bin cdn | ssh_vm 'tar -C /opt/e2e -x'
tar -C "$here" -c harness.py fake_api.py | ssh_vm 'tar -C /opt/e2e -x'
# smithd reboots a device that cannot reach its API for long; never the VM.
ssh_vm 'printf "#!/bin/sh\necho reboot \"\$@\" >> /opt/e2e/results/reboot.log\n" > /opt/e2e/shim/reboot && chmod +x /opt/e2e/shim/reboot'
ssh_vm 'sudo sh -c "nohup python3 /opt/e2e/fake_api.py /opt/e2e/cdn /opt/e2e/state 8000 > /opt/e2e/state/api.log 2>&1 &"'

run() { ssh_vm "sudo python3 /opt/e2e/harness.py $*"; }
if [ $# -gt 0 ]; then
    run "$@"
else
    run powercut --cut-with main --resume-with main --trials 24 --seed 1
    run powercut --cut-with head --resume-with head --trials 24 --seed 1
    run powercut --cut-with main --resume-with head --trials 8 --seed 2
    run faults --version main
    run faults --version head
    run dpkg --version main
    run dpkg --version head
fi
ssh_vm 'tar -C /opt/e2e -c results' | tar -C "$work" -x
