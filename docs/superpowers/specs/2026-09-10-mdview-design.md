# mdview 설계 문서 (2026-09-10)

## 목표
glow(https://github.com/charmbracelet/glow)와 같은 터미널 마크다운 뷰어를 Rust로 구현한다.

## 범위
- 입력: 파일 경로, 표준입력(`-` 또는 파이프). 인자 없이 TTY에서 실행하면 현재 디렉터리의 마크다운 파일 브라우저(TUI)를 연다.
- 출력 모드
  - 기본: 스타일이 적용된 ANSI 텍스트를 stdout으로 출력.
  - `-p/--pager`: ratatui 기반 페이저(스크롤, `/` 검색, `n/N` 이동, `q` 종료).
- 옵션: `-s/--style dark|light|notty|auto`, `-w/--width N`(기본 터미널 폭, 최대 120), `--no-color`.
- 렌더링 지원 요소: 제목(H1~H6), 문단, 강조/굵게/취소선, 인라인 코드, 코드블록(syntect 하이라이트), 순서/비순서/중첩/체크박스 목록, 인용, 표, 링크(텍스트 + URL), 이미지(대체 텍스트 + URL), 수평선, 줄바꿈. CJK 문자의 표시 폭을 고려한 단어 단위 줄바꿈.

## 아키텍처
```
src/
  main.rs        CLI 진입점. 인자 파싱 후 source → render → output 경로 선택
  cli.rs         clap 정의
  source.rs      입력 로드(파일/stdin) 및 md 파일 탐색(ignore 크레이트)
  theme.rs       Theme(dark/light/notty): 요소별 Style
  doc.rs         문서 모델: Style, Span, Line (렌더러와 출력기 사이의 공통 타입)
  wrap.rs        스타일 span 단위 단어 줄바꿈, unicode-width 기반
  render/
    mod.rs       pulldown-cmark 이벤트 → Vec<Line> (블록 상태 스택 유지)
    code.rs      syntect 하이라이트 → Span 목록
    table.rs     표 열 폭 계산과 정렬
  output/
    ansi.rs      Vec<Line> → ANSI 문자열
    pager.rs     ratatui 페이저
    picker.rs    파일 브라우저 TUI
```

## 데이터 흐름
markdown 문자열 → `render::render(md, &Theme, width)` → `Vec<Line>` → `ansi::to_string` 또는 페이저에서 ratatui `Line`으로 변환하여 표시.

## 오류 처리
anyhow로 전파, main에서 메시지 출력 후 종료 코드 1. TUI 진입 시 ratatui::init/restore로 터미널 상태 보장.

## 테스트
- wrap: ASCII/한글 혼합 폭, 긴 단어 강제 분할, 스타일 유지.
- render: 각 마크다운 요소가 기대한 평문 구조로 나오는지(스타일 제거 후 비교).
- table: 열 폭/정렬.
- ansi: 스타일 → escape 코드 변환.
