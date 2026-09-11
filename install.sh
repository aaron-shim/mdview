#!/usr/bin/env bash
# mdview 설치 스크립트: 배포판을 감지해 알맞은 방식으로 설치한다.
#   ./install.sh            # 자동 감지
#   ./install.sh cargo      # 강제로 cargo install --path .
#   ./install.sh arch|deb   # 강제로 해당 방식
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

mode="${1:-auto}"
if [ "$mode" = "auto" ]; then
  id=""; like=""
  if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    id="${ID:-}"; like="${ID_LIKE:-}"
  fi
  case " $id $like " in
    *" arch "*|*" manjaro "*|*" endeavouros "*) mode=arch ;;
    *" debian "*|*" ubuntu "*) mode=deb ;;
    *) mode=cargo ;;
  esac
fi

need() { command -v "$1" >/dev/null 2>&1 || { echo "필요한 명령이 없습니다: $1" >&2; echo "$2" >&2; exit 1; }; }

case "$mode" in
  arch)
    need makepkg "sudo pacman -S --needed base-devel"
    need cargo   "sudo pacman -S rust"
    echo ">> Arch Linux: makepkg -si"
    (cd packaging/arch && makepkg -si --clean)
    ;;
  deb)
    need cargo    "rustup 설치: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    need dpkg-deb "sudo apt install dpkg"
    need cc       "sudo apt install build-essential"
    echo ">> Debian/Ubuntu: .deb 빌드 후 apt 설치"
    ./packaging/debian/build-deb.sh
    deb="$(ls -t dist/mdview_*.deb | head -1)"
    sudo apt install "./$deb"
    ;;
  cargo)
    need cargo "rustup 설치: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo ">> cargo install --path ."
    cargo install --path . --locked
    case ":$PATH:" in
      *":$HOME/.cargo/bin:"*) ;;
      *) echo; echo "PATH에 \$HOME/.cargo/bin 을 추가하세요:"; echo '  export PATH="$HOME/.cargo/bin:$PATH"' ;;
    esac
    ;;
  *) echo "알 수 없는 모드: $mode (auto|arch|deb|cargo)" >&2; exit 2 ;;
esac

echo
echo ">> 설치 완료. 확인: mdview --version"
