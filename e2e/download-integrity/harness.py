#!/usr/bin/env python3
"""Download-integrity scenarios against the real smithd binary.

Runs as root inside a throwaway VM (see run-vm.sh). smithd talks to fake_api.py
on 127.0.0.1:8000 and downloads through its control socket or its updater, so
the real downloader, updater and apt code run. Results go to stdout as JSON
lines and to <E2E>/results.
"""
import argparse
import fcntl
import hashlib
import json
import os
import random
import re
import signal
import subprocess
import sys
import time

E2E = "/opt/e2e"
RUN = f"{E2E}/run"
STATE = f"{E2E}/state"
CDN = f"{E2E}/cdn"
SOCK = "/run/smithd/smithd.sock"
CUT_IMG = f"{E2E}/cut.img"
CUT = "/mnt/cut"
BIG = "e2e-big_1_all.deb"
FS_IOC_GOINGDOWN = 0x8004587D
EXT4_GOING_FLAGS_NOLOGFLUSH = 2


def sh(*cmd, check=True, **kw):
    return subprocess.run(cmd, check=check, capture_output=True, text=True, **kw)


def md5(path):
    h = hashlib.md5()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def emit(record):
    line = json.dumps(record, sort_keys=True)
    print(line, flush=True)
    with open(f"{E2E}/results/results.jsonl", "a") as f:
        f.write(line + "\n")


class Smithd:
    """One smithd process. kill() is SIGKILL, as a power cut stops it."""

    def __init__(self, version, magic=None):
        self.version = version
        self.log_path = f"{E2E}/results/smithd-{version}.log"
        self.log = open(self.log_path, "a")
        self.offset = self.log.tell()
        os.makedirs(RUN, exist_ok=True)
        with open(f"{RUN}/magic.toml", "w") as f:
            f.write('[meta]\nmagic_version = 2\nserver = "http://127.0.0.1:8000"\n')
            for key, value in (magic or {}).items():
                f.write(f"{key} = {value}\n")
        if os.path.exists(SOCK):
            os.remove(SOCK)
        env = dict(os.environ, RUST_LOG="info", PATH=f"{E2E}/shim:" + os.environ["PATH"])
        self.proc = subprocess.Popen(
            [f"{E2E}/bin/smithd-{version}"], cwd=RUN, env=env, stdout=self.log, stderr=self.log
        )
        deadline = time.time() + 120
        while not os.path.exists(SOCK):
            if time.time() > deadline or self.proc.poll() is not None:
                raise RuntimeError(f"smithd-{version} did not open its control socket")
            time.sleep(0.5)

    def lines(self):
        with open(self.log_path) as f:
            f.seek(self.offset)
            return re.sub(r"\x1b\[[0-9;]*m", "", f.read())

    def download(self, remote, local, rate_mb=2.0):
        body = json.dumps({"remote_file": remote, "local_file": local, "rate_mb": rate_mb})
        sh("curl", "-sf", "--unix-socket", SOCK, "-X", "POST", "http://smithd/downloads",
           "-H", "content-type: application/json", "-d", body)

    def kill(self):
        self.proc.send_signal(signal.SIGKILL)
        self.proc.wait()
        self.log.close()


def wait_download(smithd, local, timeout):
    """Waits until smithd finalizes the file or stops the download.
    Returns "finalized", "failed: <reason>" or "timeout"."""
    markers = ("Download finalized: ", "File already fully downloaded", "Download failed: ",
               "Error downloading chunk: ")
    start = {m: smithd.lines().count(m) for m in markers}
    deadline = time.time() + timeout
    while time.time() < deadline:
        text = smithd.lines()
        new = {m: text.count(m) > start[m] for m in markers}
        if (new[markers[0]] or new[markers[1]]) and os.path.exists(local):
            return "finalized"
        # smithd before this change logs only the chunk error, not the failure.
        for m in markers[2:]:
            if new[m]:
                time.sleep(1)
                return "failed: " + re.findall(re.escape(m) + "(.*)", smithd.lines())[-1]
        time.sleep(0.5)
    return "timeout"


def resume_offsets(text):
    return [int(x) for x in re.findall(r"Resuming download from byte (\d+)", text)]


def damaged_pages(path, source):
    """4 KiB pages of `path` that differ from the same range of `source`."""
    bad = []
    with open(path, "rb") as a, open(source, "rb") as b:
        index = 0
        while True:
            pa = a.read(4096)
            if not pa:
                break
            if pa != b.read(len(pa)):
                bad.append(index * 4096)
            index += 1
    return bad


def xattr(path, name):
    try:
        return os.getxattr(path, name).decode()
    except OSError:
        return None


def dpkg_install(deb):
    """Installs and removes the package. Returns (ok, last error line)."""
    r = sh("dpkg", "-i", deb, check=False)
    ok = r.returncode == 0
    err = "" if ok else next((l for l in r.stderr.splitlines() if "error" in l), r.stderr.strip())
    sh("dpkg", "--purge", "e2e-big", check=False)
    return ok, err


# --- the filesystem a power cut hits -------------------------------------


DM = "e2ecut"


def dm_table(target):
    loop = sh("losetup", "-j", CUT_IMG).stdout.split(":")[0]
    sectors = sh("blockdev", "--getsz", loop).stdout.strip()
    return f"0 {sectors} linear {loop} 0" if target == "linear" else f"0 {sectors} error"


def dm_swap(target):
    sh("dmsetup", "suspend", "--noflush", "--nolockfs", DM)
    sh("dmsetup", "load", DM, "--table", dm_table(target))
    sh("dmsetup", "resume", DM)


def mount_cut(mode):
    """ext4 with default options on a loop file; in dm-error mode through a
    device-mapper device that the cut switches to an error target."""
    os.makedirs(CUT, exist_ok=True)
    fresh = not os.path.exists(CUT_IMG)
    if fresh:
        sh("truncate", "-s", "2G", CUT_IMG)
    if mode == "shutdown":
        if fresh:
            sh("mkfs.ext4", "-q", "-F", CUT_IMG)
        sh("mount", "-o", "loop", CUT_IMG, CUT)
        return
    sh("losetup", "-f", CUT_IMG)
    sh("dmsetup", "create", DM, "--table", dm_table("linear"))
    if fresh:
        sh("mkfs.ext4", "-q", "-F", f"/dev/mapper/{DM}")
    sh("mount", f"/dev/mapper/{DM}", CUT)


def power_cut(mode):
    """shutdown: stop the filesystem with no journal flush (EXT4_IOC_SHUTDOWN,
    NOLOGFLUSH). dm-error: fail every write that has not completed, as the
    disk loses power."""
    if mode == "shutdown":
        fd = os.open(CUT, os.O_RDONLY)
        try:
            fcntl.ioctl(fd, FS_IOC_GOINGDOWN, EXT4_GOING_FLAGS_NOLOGFLUSH.to_bytes(4, "little"))
        finally:
            os.close(fd)
    else:
        dm_swap("error")


def remount(mode):
    """Unmounts while writes still fail, so no cached data reaches the disk, then
    mounts again: the journal replay a reboot does."""
    sh("umount", CUT)
    if mode == "shutdown":
        sh("mount", "-o", "loop", CUT_IMG, CUT)
    else:
        dm_swap("linear")
        sh("mount", f"/dev/mapper/{DM}", CUT)


def unmount_cut(mode):
    sh("umount", CUT, check=False)
    if mode != "shutdown":
        sh("dmsetup", "remove", DM, check=False)
        loop = sh("losetup", "-j", CUT_IMG).stdout.split(":")[0]
        if loop:
            sh("losetup", "-d", loop, check=False)


# --- scenarios -----------------------------------------------------------


def scenario_powercut(args):
    """Cut power during a download with `--cut-with`, then resume with
    `--resume-with`. Counts damaged .part files, resume offsets and results."""
    rng = random.Random(args.seed)
    source = f"{CDN}/{BIG}"
    expected = md5(source)
    size = os.path.getsize(source)
    local = f"{CUT}/{BIG}"
    mount_cut(args.cut_mode)
    try:
        for trial in range(args.trials):
            for p in (local, local + ".part"):
                if os.path.exists(p):
                    os.remove(p)
            sh("sync")
            # Writeback starts only after 30 s (dirty_expire_centisecs), so the cut
            # comes after that, as in a device download that takes minutes.
            cut_after = round(rng.uniform(35, size / (args.rate * 1e6) - 5), 1)

            smithd = Smithd(args.cut_with)
            smithd.download(f"packages/{BIG}", local, rate_mb=args.rate)
            time.sleep(cut_after)
            power_cut(args.cut_mode)
            smithd.kill()
            remount(args.cut_mode)

            part = local + ".part"
            part_len = os.path.getsize(part) if os.path.exists(part) else None
            synced = xattr(part, "user.synced_len") if part_len is not None else None
            bad = damaged_pages(part, source) if part_len else []

            smithd = Smithd(args.resume_with)
            smithd.download(f"packages/{BIG}", local)
            outcome = wait_download(smithd, local, timeout=size / 2e6 * 3 + 120)
            log = smithd.lines()
            smithd.kill()

            final_md5 = md5(local) if os.path.exists(local) else None
            dpkg_ok, dpkg_err = dpkg_install(local) if final_md5 else (None, "")
            emit({
                "scenario": "powercut", "cut_mode": args.cut_mode,
                "cut_with": args.cut_with, "resume_with": args.resume_with,
                "trial": trial, "cut_after_s": cut_after, "part_len": part_len,
                "part_len_4k_aligned": part_len is not None and part_len % 4096 == 0,
                "synced_len": synced, "damaged_pages_in_part": bad,
                "resume_offsets": resume_offsets(log), "outcome": outcome,
                "final_md5_ok": final_md5 == expected if final_md5 else None,
                "dpkg_ok": dpkg_ok, "dpkg_error": dpkg_err,
            })
    finally:
        unmount_cut(args.cut_mode)


def write_part(local, source, length, zero_at, synced_len, etag):
    with open(source, "rb") as f:
        data = bytearray(f.read(length))
    data[zero_at:zero_at + 4096] = bytes(4096)
    part = local + ".part"
    with open(part, "wb") as f:
        f.write(data)
    os.setxattr(part, "user.etag", etag.encode())
    if synced_len is not None:
        os.setxattr(part, "user.synced_len", str(synced_len).encode())


def requests_since(mark):
    with open(f"{STATE}/requests.log") as f:
        return [line.split(" ", 1)[1].strip() for line in f.read().splitlines()[mark:]]


def request_mark():
    try:
        with open(f"{STATE}/requests.log") as f:
            return len(f.read().splitlines())
    except FileNotFoundError:
        return 0


def set_fault(name, text):
    path = f"{STATE}/{name}.fault"
    if text is None:
        if os.path.exists(path):
            os.remove(path)
    else:
        with open(path, "w") as f:
            f.write(text)


def scenario_faults(args):
    """Damaged .part files and network faults, one smithd version per run."""
    source = f"{CDN}/{BIG}"
    expected = md5(source)
    etag = f'"{expected}"'
    size = os.path.getsize(source)
    workdir = f"{E2E}/faults"
    os.makedirs(workdir, exist_ok=True)
    local = f"{workdir}/{BIG}"
    zero_at = 20 * 1024 * 1024 - 4096

    def clean():
        for p in (local, local + ".part"):
            if os.path.exists(p):
                os.remove(p)
        set_fault(BIG, None)

    def run(name, prepare, attempts=1, timeout=None):
        clean()
        prepare()
        mark = request_mark()
        smithd = Smithd(args.version)
        outcomes = []
        for _ in range(attempts):
            smithd.download(f"packages/{BIG}", local)
            outcomes.append(wait_download(smithd, local, timeout or size / 2e6 * 3 + 120))
            if outcomes[-1] == "finalized":
                break
        log = smithd.lines()
        smithd.kill()
        final_md5 = md5(local) if os.path.exists(local) else None
        emit({
            "scenario": name, "version": args.version, "outcomes": outcomes,
            "blob_requests": requests_since(mark), "resume_offsets": resume_offsets(log),
            "final_md5_ok": final_md5 == expected if final_md5 else None,
            "part_left": os.path.exists(local + ".part"),
        })

    # A .part as an older smithd leaves it after a power cut: zero page inside
    # the length, ETag xattr, no synced length.
    run("part_zero_page_no_synced_len",
        lambda: write_part(local, source, 24 * 1024 * 1024, zero_at, None, etag))
    # A synced length that covers a zero page (the data lied): only the MD5 check can catch it.
    run("part_zero_page_inside_synced_len",
        lambda: write_part(local, source, 24 * 1024 * 1024, zero_at, 24 * 1024 * 1024, etag),
        attempts=2)
    # A synced length below the zero page: the zero page is truncated away.
    run("part_zero_page_after_synced_len",
        lambda: write_part(local, source, 24 * 1024 * 1024, zero_at, 16 * 1024 * 1024, etag))
    # The connection resets mid-body; the next request resumes.
    run("network_reset_mid_body", lambda: set_fault(BIG, "drop 30000123"), attempts=2)
    # The connection stalls mid-body; the read timeout ends it, the next request resumes.
    run("network_stall_mid_body", lambda: set_fault(BIG, "stall 30000123"), attempts=2,
        timeout=args.stall_timeout)
    # The server reports an ETag that does not match the content.
    run("wrong_etag", lambda: set_fault(BIG, "etag 0123456789abcdef0123456789abcdef"))
    clean()


DPKG_PACKAGES = ("snakebrain", "rfdetr-orin", "rfdetr-model", "bell", "smith")


def dpkg_states():
    r = sh("dpkg-query", "-W", "-f=${db:Status-Abbrev}|${Package}|${Version}\\n",
           *DPKG_PACKAGES, check=False)
    states = {}
    for line in r.stdout.splitlines():
        status, name, version = line.split("|")
        if status.strip() not in ("un", "pn"):
            states[name] = f"{status.strip()} {version}"
    return states


def write_release(release_id, packages):
    with open(f"{STATE}/release-{release_id}.json", "w") as f:
        json.dump([{"name": n, "version": v, "file": f"{n}_{v}_all.deb"} for n, v in packages], f)


def break_dpkg():
    """The state of 1421626010227: release 1 installed, then a batch whose
    rfdetr-model blob is corrupt leaves the new snakebrain unpacked (iU) with
    its dependency missing."""
    sh("dpkg", "--purge", *DPKG_PACKAGES, check=False)
    sh("dpkg", "-i", *(f"{CDN}/{n}_1_all.deb" for n in ("snakebrain", "rfdetr-orin", "bell", "smith")))
    # apt takes an argument as a file only when it ends in .deb.
    os.makedirs(f"{E2E}/corrupt", exist_ok=True)
    corrupt = f"{E2E}/corrupt/rfdetr-model_2_all.deb"
    sh("cp", f"{CDN}/rfdetr-model_2_all.corrupt", corrupt)
    r = sh("apt", "install", "-y", "--allow-downgrades", f"{CDN}/snakebrain_2_all.deb",
           corrupt, f"{CDN}/bell_2_all.deb", check=False)
    sh("rm", "-rf", f"{RUN}/packages")
    return r.returncode, [l for l in r.stderr.splitlines() if "error" in l.lower()][:2]


def run_updater_cycle(version, release_id):
    smithd = Smithd(version, {"release_id": 1, "target_release_id": release_id})
    # The first cycle fetches the blobs and the next one writes the manifest and installs.
    deadline = time.time() + 900
    while time.time() < deadline:
        text = smithd.lines()
        if "Installing " in text and text.count("Upgrading result:") >= 2:
            break
        time.sleep(1)
    log = smithd.lines()
    smithd.kill()
    keep = ("Installing ", "Successfully installed", "Unmet dependencies", "Depends:",
            "remove is disabled", "Upgrading result", "failed ETag")
    return [l.split("  ", 1)[-1][:300] for l in log.splitlines() if any(k in l for k in keep)]


def run_smith_updater(version, release_id):
    """smith-updater reads /etc/smith/packages, so that is its own directory here."""
    home = f"{E2E}/updater"
    sh("rm", "-rf", home)
    os.makedirs(f"{home}/packages/versions")
    os.makedirs(f"{home}/packages/blobs")
    with open(f"{home}/magic.toml", "w") as f:
        f.write(f'[meta]\nmagic_version = 2\nserver = "http://127.0.0.1:8000"\n'
                f"target_release_id = {release_id}\n")
    with open(f"{home}/packages/versions/{release_id}", "w") as f:
        f.write("smith 2 smith_2_all.deb\n")
    sh("cp", f"{CDN}/smith_2_all.deb", f"{home}/packages/blobs/")
    os.makedirs("/etc/smith", exist_ok=True)
    if os.path.islink("/etc/smith/packages"):
        os.remove("/etc/smith/packages")
    os.symlink(f"{home}/packages", "/etc/smith/packages")
    r = sh(f"{E2E}/bin/smith-updater-{version}", cwd=home, check=False,
           env=dict(os.environ, RUST_LOG="info"))
    keep = ("Smith installed", "Failed to install smith", "Depends:", "remove is disabled",
            "Unmet dependencies", "Error")
    out = re.sub(r"\x1b\[[0-9;]*m", "", r.stdout + r.stderr)
    return [l.split("  ", 1)[-1][:300] for l in out.splitlines() if any(k in l for k in keep)]


def scenario_dpkg(args):
    v = args.version
    ok_release = [("snakebrain", "2"), ("rfdetr-model", "2"), ("bell", "2"), ("smith", "2")]
    write_release(10, ok_release)
    # A release without the provider: apt can only resolve it by removing snakebrain.
    write_release(11, [("snakebrain", "2"), ("bell", "2")])

    code, err = break_dpkg()
    broken = dpkg_states()
    emit({"scenario": "dpkg_broken_state", "version": v, "apt_exit": code, "apt_error": err,
          "states": broken})
    emit({"scenario": "smith_updater_on_broken_state", "version": v,
          "log": run_smith_updater(v, 10), "states_before": broken, "states": dpkg_states()})
    before = dpkg_states()
    emit({"scenario": "smithd_batch_on_broken_state", "version": v,
          "log": run_updater_cycle(v, 10), "states_before": before, "states": dpkg_states()})
    before = dpkg_states()
    emit({"scenario": "smith_updater_after_batch", "version": v,
          "log": run_smith_updater(v, 10), "states_before": before, "states": dpkg_states()})

    break_dpkg()
    before = dpkg_states()
    emit({"scenario": "smithd_batch_without_provider", "version": v,
          "log": run_updater_cycle(v, 11), "states_before": before, "states": dpkg_states()})
    sh("dpkg", "--purge", *DPKG_PACKAGES, check=False)


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("powercut")
    p.add_argument("--cut-with", required=True)
    p.add_argument("--resume-with", required=True)
    p.add_argument("--trials", type=int, default=20)
    p.add_argument("--seed", type=int, default=1)
    p.add_argument("--cut-mode", choices=("shutdown", "dm-error"), default="shutdown")
    p.add_argument("--rate", type=float, default=0.5, help="MB/s of the download that is cut")
    f = sub.add_parser("faults")
    f.add_argument("--version", required=True)
    f.add_argument("--stall-timeout", type=int, default=150)
    d = sub.add_parser("dpkg")
    d.add_argument("--version", required=True)
    args = parser.parse_args()
    os.makedirs(f"{E2E}/results", exist_ok=True)
    {"powercut": scenario_powercut, "faults": scenario_faults, "dpkg": scenario_dpkg}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
