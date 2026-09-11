#!/usr/bin/env bash
# mdview .deb 패키지 빌드 스크립트 (Debian/Ubuntu).
#
#   ./packaging/debian/build-deb.sh            # 현재 머신용 .deb 생성
#   sudo apt install ./dist/mdview_<ver>_<arch>.deb
#
# 필요한 것: cargo(rustup 권장), build-essential(gcc), dpkg-deb(기본 포함).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root"

pkgname=mdview
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if command -v dpkg >/dev/null 2>&1; then
  # macOS 의 dpkg 는 "darwin-amd64" 처럼 OS 접두어를 붙이므로 마지막 부분만 쓴다.
  arch="$(dpkg --print-architecture)"; arch="${arch##*-}"
else
  case "$(uname -m)" in
    x86_64) arch=amd64 ;;
    aarch64|arm64) arch=arm64 ;;
    armv7l) arch=armhf ;;
    *) arch="$(uname -m)" ;;
  esac
fi
# 실제 주소는 빌드할 때 DEB_MAINTAINER 로 넘긴다.
#   DEB_MAINTAINER="이름 <메일>" ./packaging/debian/build-deb.sh
maintainer="${DEB_MAINTAINER:-mdview maintainers <noreply@example.com>}"

# CARGO_TARGET_DIR 를 존중한다.
target_dir="${CARGO_TARGET_DIR:-target}"
bin="$target_dir/release/$pkgname"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  echo ">> building release binary"
  cargo build --release --locked
fi
[ -x "$bin" ] || { echo "binary not found: $bin" >&2; exit 1; }

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
pkgroot="$stage/${pkgname}_${version}_${arch}"

echo ">> staging files"
install -d "$pkgroot/DEBIAN" "$pkgroot/usr/bin" "$pkgroot/usr/share/doc/$pkgname"
install -m755 "$bin" "$pkgroot/usr/bin/$pkgname"
install -m644 README.md "$pkgroot/usr/share/doc/$pkgname/README.md"
install -m644 LICENSE "$pkgroot/usr/share/doc/$pkgname/copyright"

installed_size="$(du -sk "$pkgroot/usr" | cut -f1)"
cat > "$pkgroot/DEBIAN/control" <<CTRL
Package: $pkgname
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: $maintainer
Installed-Size: $installed_size
Depends: libc6, libgcc-s1
Description: Terminal markdown viewer like glow, written in Rust
 Renders markdown files, stdin, or remote URLs with colors in the terminal.
 Typesets LaTeX math, mermaid diagrams and merged HTML tables as terminal
 characters. Includes a scrollable pager with search, a markdown file browser,
 and a local stash (favorites) of documents.
CTRL

mkdir -p dist
out="dist/${pkgname}_${version}_${arch}.deb"
echo ">> building $out"
dpkg-deb --root-owner-group --build "$pkgroot" "$out" >/dev/null
echo ">> done: $out"
echo "   install with: sudo apt install ./$out"
