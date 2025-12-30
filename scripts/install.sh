#!/bin/sh
# Based on Deno installer: Copyright 2019 the Deno authors. All rights reserved. MIT license.
# TODO(everyone): Keep this script simple and easily auditable.

set -e

if ! command -v unzip >/dev/null; then
  echo "Error: unzip is required to install Smith CLI." 1>&2
  exit 1
fi

if [ "$OS" = "Windows_NT" ]; then
  target="x86_64-pc-windows-msvc"
else
  case $(uname -sm) in
  "Darwin x86_64") target="x86_64-apple-darwin" ;;
  "Darwin arm64") target="aarch64-apple-darwin" ;;
  "Linux aarch64") target="aarch64-unknown-linux-gnu" ;;
  *) target="x86_64-unknown-linux-gnu" ;;
  esac
fi

if [ $# -eq 0 ]; then
  sm_uri="https://github.com/Teton-ai/smith/releases/latest/download/sm-${target}.zip"
else
  sm_uri="https://github.com/Teton-ai/smith/releases/latest/download/sm-${target}.zip"
fi

smith_install="${SMITH_CLI_INSTALL:-$HOME/.smith}"
bin_dir="$smith_install/bin"
exe="$bin_dir/sm"

if [ ! -d "$bin_dir" ]; then
  mkdir -p "$bin_dir"
fi

curl --fail --location --progress-bar --output "$exe.zip" "$sm_uri"
unzip -d "$bin_dir" -o "$exe.zip"
chmod +x "$exe"
rm "$exe.zip"

echo "SMITH CLI was installed successfully to $exe"

if command -v sm >/dev/null; then
  echo "Run 'sm --help' to get started"
else
  case $SHELL in
  /bin/zsh) shell_profile=".zshrc" ;;
  */fish) shell_profile=".config/fish/config.fish" ;;
  *) shell_profile=".bashrc" ;;
  esac

  if [ "$SHELL" = "*/fish" ] || [ "$(basename "$SHELL")" = "fish" ]; then
    echo "Manually add the directory to your \$HOME/$shell_profile"
    echo "  set -Ux SMITH_CLI_INSTALL \"$smith_install\""
    echo "  fish_add_path \$SMITH_CLI_INSTALL/bin"
  else
    echo "Manually add the directory to your \$HOME/$shell_profile (or similar)"
    echo "  export SMITH_CLI_INSTALL=\"$smith_install\""
    echo "  export PATH=\"\$SMITH_CLI_INSTALL/bin:\$PATH\""
    echo "Run '$exe --help' to get started"
  fi
fi
echo
