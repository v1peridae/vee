#!/bin/sh
set -e

if [ -z "$HOME" ]; then
  HOME="$(cd ~ && pwd)"
fi
INSTALL_DIR="${HOME}/.local/bin"
OS=$(uname -s)
ARCH=$(uname -m)
case "$ARCH" in
  arm64) ARCH="aarch64" ;;
esac

case "$OS" in
  Darwin)
    case "$ARCH" in
      x86_64)  ASSET="vee-macos-x86_64" ;;
      aarch64) ASSET="vee-macos-aarch64" ;;
      *)       echo "we don't have a build for $ARCH yet"; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64)  ASSET="vee-linux-x86_64" ;;
      aarch64) ASSET="vee-linux-aarch64" ;;
      *)       echo "we don't have a build for $ARCH yet"; exit 1 ;;
    esac
    ;;
  *)
    echo "sorry, vee doesn't support $OS yet"
    echo "vee is currently available only on linux and macos."
    exit 1
    ;;
esac

echo "fetching the latest vee release..."
TAG=$(curl -sL "https://api.github.com/repos/v1peridae/vee/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$TAG" ]; then
  echo "couldn't fetch the latest release. check https://github.com/v1peridae/vee/releases"
  exit 1
fi

DOWNLOAD_URL="https://github.com/v1peridae/vee/releases/download/${TAG}/${ASSET}"
echo "installing vee ${TAG}... :3"

if [ -z "$INSTALL_DIR" ]; then
  echo "error: could not determine the install directory"
  exit 1
fi
mkdir -p "$INSTALL_DIR"

curl -sSL "$DOWNLOAD_URL" -o "${INSTALL_DIR}/vee"
chmod +x "${INSTALL_DIR}/vee"

if [ "$OS" = "Darwin" ]; then
  xattr -d com.apple.quarantine "${INSTALL_DIR}/vee" 2>/dev/null || true
fi

echo ""
echo "vee has been installed to ${INSTALL_DIR}/vee"

if echo "$PATH" | grep -q "${INSTALL_DIR}"; then
  echo ""
  echo "run 'vee --help' to get started!"
  exit 0
fi
if [ -n "$ZSH_VERSION" ] || [ -f "${HOME}/.zshrc" ]; then
  SHELL_RC="${HOME}/.zshrc"
elif [ -n "$BASH_VERSION" ] || [ -f "${HOME}/.bashrc" ]; then
  SHELL_RC="${HOME}/.bashrc"
else
  SHELL_RC="${HOME}/.profile"
fi

PATH_LINE='export PATH="$HOME/.local/bin:$PATH"'
if [ -f "$SHELL_RC" ] && grep -q '.local/bin' "$SHELL_RC"; then
  echo ""
  echo "run 'vee --help' to get started!"
  exit 0
fi

echo "" >> "$SHELL_RC"
echo "# vee" >> "$SHELL_RC"
echo "$PATH_LINE" >> "$SHELL_RC"
echo ""
echo "added ${INSTALL_DIR} to your PATH in $SHELL_RC"
echo "run 'source $SHELL_RC' or open a new terminal, then 'vee --help' to get started!"
