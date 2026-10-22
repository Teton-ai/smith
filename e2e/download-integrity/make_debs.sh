#!/bin/bash
# Builds the test packages into $1 (run on Ubuntu 22.04 for its dpkg-deb).
#  e2e-big: about 64 MiB of zstd data, so a damaged page fails decompression.
#  The rest copy the dependency shape of the stuck device 1421626010227: the new
#  snakebrain depends on a name that only the new rfdetr-model provides.
set -euo pipefail
out=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

deb() { # name version [control lines...]
    local name=$1 version=$2 dir=$work/$1_$2
    shift 2
    mkdir -p "$dir/DEBIAN" "$dir/opt/e2e/$name"
    {
        echo "Package: $name"
        echo "Version: $version"
        echo "Architecture: all"
        echo "Maintainer: e2e <e2e@example.com>"
        echo "Description: smith download-integrity test package"
        for line in "$@"; do echo "$line"; done
    } > "$dir/DEBIAN/control"
    if [ ! -e "$dir/opt/e2e/$name/payload" ]; then
        head -c 1048576 /dev/urandom | base64 > "$dir/opt/e2e/$name/payload"
    fi
    dpkg-deb -Zzstd -z3 --root-owner-group -b "$dir" "$out/${name}_${version}_all.deb" > /dev/null
}

mkdir -p "$out" "$work/e2e-big_1/opt/e2e/e2e-big"
head -c 50000000 /dev/urandom | base64 > "$work/e2e-big_1/opt/e2e/e2e-big/payload"
deb e2e-big 1

deb snakebrain 1
deb rfdetr-orin 1
deb bell 1
deb smith 1
deb snakebrain 2 "Depends: rfdetr-weights-3944, bell (>= 2)"
deb rfdetr-model 2 "Provides: rfdetr-weights-3944"
deb bell 2
deb smith 2

# The same rfdetr-model with one page of zeros in its data, as a power cut leaves it.
cp "$out/rfdetr-model_2_all.deb" "$out/rfdetr-model_2_all.corrupt"
size=$(stat -c %s "$out/rfdetr-model_2_all.corrupt")
dd if=/dev/zero of="$out/rfdetr-model_2_all.corrupt" bs=4096 seek=$((size / 4096 / 2)) count=1 conv=notrunc status=none
