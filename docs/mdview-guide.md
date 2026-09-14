# mdview 개발 정리 및 설치 안내

버전: 0.3.4 · 갱신일: 2026-09-14 (최초 작성 2026-09-10)

## 1. 개요

mdview는 [glow](https://github.com/charmbracelet/glow)와 같은 터미널 마크다운 뷰어를 Rust로 구현한 프로그램입니다.
마크다운 파일, 표준입력, 원격 URL을 색과 스타일을 입혀 터미널에 출력하고, 스크롤·검색이 되는 페이저와
마크다운 파일 브라우저, 로컬 즐겨찾기(스태시)를 제공합니다.
0.2.0부터 수식(LaTeX), HTML 표 병합, mermaid 다이어그램을 터미널 문자로 조판하고,
0.3.0부터 파일 브라우저에 미리보기 창과 디렉터리 트리 보기를 제공합니다.

| 항목 | 내용 |
|------|------|
| 버전 | 0.3.4 |
| 언어 / 에디션 | Rust 2024, **최소 rustc 1.88** (`Cargo.toml`의 `rust-version`) |
| 라이선스 | MIT |
| 바이너리 이름 | `mdview` |
| 지원 OS | macOS, Linux (Arch, Ubuntu/Debian 등). 시스템 라이브러리 의존 없음 |
| 주요 크레이트 | pulldown-cmark(파싱), syntect(구문 강조), ratatui + crossterm(TUI), ureq(HTTP), ignore(파일 탐색), serde_json(스태시 저장) |

## 2. 기능

### 렌더링
- 제목 H1~H6: `#` 기호를 지우고 렌더링. H1은 아래에 굵은 줄(`━`), H2는 옅은 줄(`─`)을 제목 너비만큼 긋고,
  H3 이하는 굵기와 색만 단계적으로 낮춘다. 색 없는 출력에서도 줄로 구조가 남는다
- 문단, 강조, 굵게, 취소선, 인라인 코드, 강제 줄바꿈
- 코드블록: syntect 구문 강조 (언어 별칭 rs/py/sh/js/ts 등 지원, 탭은 4칸으로 확장)
- 목록: 순서/비순서/중첩(깊이별 기호 • ◦ ▪)/체크박스(✓ ☐), 느슨한 목록 사이 빈 줄
- 인용(`│` 막대), 표(정렬, 셀 줄바꿈, 폭 자동 축소), 수평선
- 링크 `텍스트 (URL)`, 이미지 `🖼 대체텍스트 (URL)`, 각주(문서 끝에 모아 출력)
- YAML 프론트매터 생략, HTML은 흐리게 그대로 출력
- 줄바꿈: unicode-width 기반. 한글은 어절(공백) 단위, 한자/가나는 글자 단위, 긴 단어는 강제 분할
- 한글 자소 결합: macOS가 나눠 저장한 한글(NFD)을 읽어 들일 때 음절로 합친다(`hangul.rs`).
  유니코드 표준 조합 알고리즘을 그대로 쓰며 한글 외 결합 문자는 건드리지 않는다.
  적용 범위: 파일 브라우저의 Local·Stashed 목록(파일 이름, 디렉터리, 메모, 이름 필터), 페이저 제목,
  파일·표준입력·URL로 읽은 문서 내용. 저장·조회에 쓰는 실제 경로와 스태시 키는 건드리지 않는다
  (파일 시스템이 나뉜 이름을 쓰고 있으면 합친 이름으로는 열리지 않는다)

### 수식 (LaTeX)
`$...$`(인라인)와 `$$...$$`(블록)을 유니코드 문자로 조판합니다. 외부 도구는 쓰지 않습니다.

- 분수(`\frac`, `\dfrac`), 근호(`\sqrt`), 이항계수(`\binom`)
- 첨자: 짧으면 유니코드 첨자(`x_i` → `xᵢ`, `2^{10}` → `2¹⁰`), 낱말이면 밑줄 표기(`T_{total}` → `T_total`),
  그 밖에는 한 줄 올리거나 내려서 배치
- 환경: `matrix`·`pmatrix`·`bmatrix`·`Bmatrix`·`vmatrix`·`cases`·`aligned`·`array`(열 정렬 지정 포함)
- 큰 연산자 `\sum`·`\prod`·`\lim` 등은 한계를 위아래로, `\int` 계열은 오른쪽 첨자로
- 강세 `\overline`·`\hat`·`\vec`, 글꼴 `\mathbb`·`\mathcal`·`\mathbf`, mhchem `\ce{}`
- `\left(` … `\right]` 는 내용 높이에 맞춰 늘어난 괄호(`⎛⎜⎝`, `⎡⎢⎣`, `⎧⎨⎩`)로
- 연산자·관계 기호 앞뒤에 공백을 넣어 읽기 좋게 하되, 첨자 안에서는 붙여 씁니다
- 폭이 넘치면 한 줄 표기(`a/b`)로 낮추고, 그래도 넘치면 줄을 접습니다. 모르는 명령은 이름을 남깁니다

### 도표
- 마크다운 표: 정렬, 셀 줄바꿈, 폭 자동 축소 (기존 기능)
- 셀 안 `<br>`은 실제 줄바꿈으로 처리 (한 칸이 여러 줄을 가질 수 있음)
- HTML `<table>`: `rowspan`·`colspan` 병합, `<thead>`/`<tbody>`/`<caption>`, 칸 안의 태그 제거와 실체 해제.
  칸마다 테두리를 그리고 겹치는 선은 이음 문자(`┬` `┼` `┤`)로 합칩니다

### Mermaid 다이어그램
` ```mermaid ` 코드블록을 문자 그림으로 그립니다. 지원하지 않는 종류이거나 터미널 폭에 들어가지 않으면
원문 코드블록을 그대로 보여줍니다.

| 종류 | 내용 |
|------|------|
| `flowchart`, `graph` | 노드 모양(사각·둥근·마름모·원통·서브루틴), `subgraph` 테두리, 실선·점선·굵은 선, 간선 라벨, 되돌아가는 간선, 자기 자신 간선 |
| `sequenceDiagram` | `participant`/`actor`, `A->>B: 글` 화살표(실선·점선·`-x`), `alt`/`else`/`opt`/`loop`/`par` 틀, `Note over`, `autonumber` |
| `stateDiagram(-v2)` | `[*]` 시작(●)·끝(◉), 전이 라벨, 합성 상태(`state X { }`), `note ... end note` |
| `erDiagram` | 엔터티 상자(속성 `이름 : 타입 [KEY]`), 관계 카디널리티 `1`·`0..1`·`0..N`·`1..N`, 식별/비식별(점선) |
| `classDiagram` | 이름·«스테레오타입»·속성·메서드 칸, 상속 `<\|--`(▽)·합성 `*--`(◆)·집합 `o--`(◇)·연관 `-->`·의존 `..>` |
| `gantt` | `dateFormat`, `section`, `after <id>` 연결, 기간 `5d`/`2w`/`12h`, 날짜 축과 막대(▒ 완료 / ▓ 진행 / █ 예정 / ◆ 마일스톤) |
| `pie` | 가로 막대, 백분율, `showData`면 값과 합계 |
| `gitGraph` | 브랜치 레인, `commit`(●), `branch`, `checkout`, `merge`(◆), `tag` |

그래프 계열(flowchart·state·er·class)은 공통 배치기 `mermaid/graph.rs`를 씁니다.

1. **층 나누기** — 서브그래프를 한 덩어리로 보고 덩어리끼리 먼저 층을 매겨 겹치지 않는 층 구간을 나눈 뒤,
   덩어리 안에서 다시 최장 경로로 층을 매깁니다. 되돌아가는 간선은 DFS로 찾아내 빼고 계산합니다.
2. **순서 정하기** — 무게중심(barycenter) 4회 반복, 같은 서브그래프 구성원은 한데 모읍니다.
3. **자리 잡기** — 서브그래프 안에서 상대 위치를 먼저 정하고, 그 덩어리를 블록으로 보아 층마다 왼쪽부터
   늘어놓습니다. 여러 층에 걸친 블록은 모든 층에서 같은 자리를 차지하므로 테두리가 포개지지 않습니다.
4. **배선** — 층 사이 "띠"에 간선을 넣습니다. 가로 구간이 겹치지 않게 구간 색칠로 줄을 나누고,
   두 층 이상 건너뛰는 간선은 가상 노드로 쪼갭니다. 되돌아가는 간선은 오른쪽 세로 통로를 타고 올라가
   목적지 옆구리(◀)로 들어갑니다.

터미널은 세로로 길고 가로로 좁으므로 `LR`/`TB` 지정과 무관하게 항상 위에서 아래로 배치합니다.
라벨 폭은 30 → 24 → 20 → 16 → 12 → 9 순으로 줄여 가며 터미널 폭에 맞는 배치를 찾습니다.

### 출력 모드
- 기본: 출력이 터미널이면 페이저로 연다. 스크롤, 검색(`/`, `n`, `N`, 대소문자 무시, 일치 부분 강조),
  마우스 휠, 창 크기 변경 시 재렌더링
- `-P, --print`(별칭 `--no-pager`): 페이저 없이 ANSI 문자열을 stdout에 바로 출력.
  파이프·리다이렉션이면 이 모드가 자동으로 쓰이고 색도 제거된다
- `-p, --pager`: 페이저를 명시적으로 요청(기본값이라 사실상 호환용)
- 인자 없이 실행: 현재 디렉터리의 마크다운 파일 브라우저 (`.gitignore`·숨김 파일 제외, 6단계 깊이)

### 파일 브라우저
- 목록: 디렉터리 트리가 기본. `v`로 경로를 한 줄씩 나열하는 평면 보기와 오간다.
  트리는 각 단계에서 디렉터리를 먼저, 그다음 파일을 이름순으로 놓고, 디렉터리 옆에 하위 파일 수를 적는다.
  `h`/`l`(또는 `Enter`)로 접고 펴며, 접힘 상태는 세션 동안 유지된다.
  필터를 걸면 결과가 숨지 않도록 접힌 곳도 모두 펼쳐 보여 준다.
- 이동: vi 키. `j`/`k`, `Ctrl-d`/`Ctrl-u`(반 페이지), `Ctrl-f`/`Ctrl-b`(한 페이지), `gg`/`G`.
  `gg`는 `g`를 두 번 눌러야 하고, 다른 키가 들어오면 대기가 풀린다. 페이지 크기는 마지막에 그린 목록 높이를 쓴다.
- 미리보기: `p`로 켜고 끈다(기본 켬). 오른쪽 창에 선택한 문서를 **실제 렌더러로** 그리므로
  수식·표·다이어그램까지 그대로 보인다. 같은 (경로, 폭)이면 다시 렌더링하지 않고 캐시를 쓴다.
  창 폭이 76칸 미만이면 자동으로 접고, 1MB가 넘는 파일과 원격 URL은 열기 전까지 렌더링하지 않는다.
- 미리보기 안으로 들어가기: `l`을 누르면 화면 구성은 그대로 둔 채 키 입력만 오른쪽 창이 받는다.
  목록 선택 표시가 `▌`에서 `│`로 흐려지고 미리보기 경계선이 또렷해져, 어느 창을 움직이는지 보인다.
  같은 이동 키가 문서를 스크롤하고(`gg`/`G` 포함), `h`·`Esc`로 목록에 돌아온다.
  푸터에 현재 위치가 백분율로 나온다. 다른 문서를 고르면 스크롤은 맨 위로 돌아간다
  (캐시 키가 바뀌는 지점에서 한 번만 처리하므로 어디서 선택이 바뀌든 빠뜨리지 않는다).
  `l`은 미리보기가 꺼져 있으면 켜고 들어가고, 창이 좁아 열 수 없으면 그 이유를 알린다.
- `h`/`l`은 문맥에 따라 이어진다(0.3.2): 열린 디렉터리의 `h`는 접기, 접힌 디렉터리·파일 위의 `h`는 상위 디렉터리로,
  접힌 디렉터리의 `l`은 펴기, 열린 디렉터리의 `l`은 첫 하위로, 파일 위의 `l`은 미리보기 창으로.
  `H`/`L`은 모든 디렉터리 접기·펴기.
- 상위 폴더: 목록 맨 위 `../` 행(`Enter`) 또는 `Backspace`·`-`. 올라온 뒤에는 방금 있던 폴더에 커서를 두고,
  그 폴더만 펼친 채 형제 폴더는 접어 둔다(접어 두었던 하위 폴더는 새 루트 기준 경로로 옮겨 유지).
  넓은 폴더도 화면이 멈추지 않도록 백그라운드에서 훑고, 그동안 푸터에 `훑는 중…`을 띄운다.
  0.3.4부터는 올라온 직후 방금 보던 하위 트리(이전 목록에 폴더 이름을 붙인 것)를 즉시 보여 주고,
  훑기는 `scan_markdown_files_with`가 64개 파일 또는 150ms마다 부분 목록을 넘겨 `done: false` 스냅숏으로
  흘려보낸다. 부분 결과는 비교 기준(`baseline`)으로 삼지 않아 변경 표시가 잘못 붙지 않는다. 훑는 도중
  새 루트 요청이 오면 감시 스레드가 그 자리에서 중단하고 마지막 요청만 훑는다(`-` 연타 대응).
  `../` 행은 필터 중에는 숨기며, 행이 생기거나 사라져도 커서는 같은 항목을 가리키도록 보정한다.
- 경로 입력 이동(0.3.4): `c`로 푸터에 `cd:` 입력창을 연다. `resolve_dir`가 `~`·상대 경로를 풀어
  `canonicalize`한 뒤 폴더인지 확인하고, `Tab`은 `complete_dir`가 하위 폴더 이름을 채운다(유일하면 `/`까지,
  여럿이면 공통 접두어, 숨김 폴더는 `.`을 직접 칠 때만). 루트 전환은 상위 이동과 같은 `set_root`를 쓴다.
  잘못된 경로면 푸터에 이유를 띄우고 입력창을 유지한다. Stashed 탭에서는 열리지 않는다.
- 자동 갱신: 감시 스레드(`output/watch.rs`)가 루트를 주기적으로 다시 훑어, 파일 목록이나
  (수정 시각, 크기) 표식이 달라졌을 때만 스냅숏을 보낸다. 브라우저는 이를 받아
  - 목록을 바꾸되 커서는 경로로 되찾아 같은 파일에 둔다(지워졌으면 제자리)
  - 새 파일 `● 새 파일`, 고친 파일 `● 변경됨`을 30초 동안 붙이고, 접힌 폴더에는 `●`만 붙인다
  - 푸터에 `새 파일: …` / `변경됨: …` / `목록 갱신: 추가 2, 삭제 1` 요약을 띄운다
  - 루트를 바꾼 직후의 첫 결과는 비교 기준으로만 쓰고 표시하지 않는다. 늦게 도착한 옛 루트 결과는 버린다
- 미리보기 갱신: 캐시 키를 (경로, 폭) + 파일 표식으로 두어, 같은 문서가 고쳐지면 다시 그리되
  스크롤 위치는 지킨다(다른 문서를 고를 때만 맨 위로). 내용이 짧아지면 스크롤을 끝에 맞춘다.
- 페이저 갱신: 파일로 연 문서는 250ms마다 표식을 확인해 바뀌면 다시 읽는다. 스크롤·검색어는 유지.
  편집기가 저장하며 파일을 잠깐 지웠다 만드는 경우를 위해, 파일이 안 보이는 순간에는 기다린다.
- 감시 방식: inotify·FSEvents 대신 폴링. OS별 차이나 감시 개수 제한이 없다.
  재검사 간격은 훑는 데 걸린 시간의 8배를 1초~15초로 자른다(측정: 이 프로젝트 0.07초 → 1초,
  홈 디렉터리 0.65초 → 약 5초). `same_file_system`으로 다른 파일 시스템에는 내려가지 않는다.
- 테마: `-s auto|dark|light|notty`. auto는 TTY면 dark, `COLORFGBG`로 밝은 배경 감지.
  `-s`를 안 주면 `MDVIEW_STYLE` 환경변수를 본다(macOS 터미널은 `COLORFGBG`를 안 내보내므로 여기서 지정).
  `--no-color`와 `NO_COLOR` 지원
- 폭: `-w N`. 기본은 터미널 폭(최대 120)

### 색 구성
바탕은 무채색, 강조는 차분한 푸른 계열 한 가지로 통일한다.

| 요소 | dark | light |
|------|------|-------|
| 본문 | 252 | 237 |
| 제목 H1·H2 / H3 | 110 / 109 | 25 / 24 |
| 제목 H4~H6 | 245 | 241 |
| 링크·인라인 코드·표 머리 | 110 | 25 |
| 목록 기호·다이어그램 테두리 | 67 | 67 |
| URL·메모 | 66 | 66 |
| 인용·수평선·표 테두리·HTML | 240~245 | 243~250 |

코드블록은 syntect 결과를 그대로 쓰지 않고 `muted_syntax` 플래그에 따라 눌러서 표시한다.
색마다 밝기(luminance)만 남기고, 원래 채도가 높았던 토큰만 푸른 쪽으로 조금 기울인다.
주석·문자열·키워드의 밝기 차이는 그대로여서 여전히 구분된다.
페이저와 파일 브라우저의 UI(선택 항목, 검색 강조, 배지)도 같은 계열을 쓴다.

### 스태시(로컬 즐겨찾기)
glow의 클라우드 스태시를 로컬 파일로 대체한 것입니다.
- 저장 위치: `$MDVIEW_STASH` > `$XDG_CONFIG_HOME/mdview/stash.json` > `~/.config/mdview/stash.json`
- 항목: 파일 절대 경로 또는 URL, 메모, 저장 시각
- 페이저 `s` 저장, 브라우저 Local 탭 `s` 저장, Stashed 탭 `x` 삭제·`m` 메모 편집·`Enter` 열기
- 명령줄: `mdview stash FILE|URL [-n 메모]`, `mdview stash --list`, `mdview stash --remove FILE|URL`

### 원격 URL
- `mdview https://…` 로 HTTP(S) 문서 읽기 (rustls + 내장 인증서, 제한 시간 15초)
- `github.com/{owner}/{repo}` → 저장소 README, `github.com/{owner}/{repo}/blob/{ref}/{path}` → raw 파일로 자동 변환
- 스킴 없는 `github.com/...`, `gitlab.com/...` 형태도 허용

## 3. 아키텍처

```
src/
  main.rs            CLI 진입점, 입력 판별, 페이저/브라우저 진입, stash 서브커맨드
  cli.rs             clap 정의 (옵션, stash 서브커맨드)
  source.rs          파일/stdin/URL 로드, GitHub URL 변환, 마크다운 파일 탐색
  stash.rs           스태시 JSON 저장소
  theme.rs           dark/light/notty 테마
  doc.rs             문서 모델: Color, Style, Span, Line
  hangul.rs          한글 자소 결합 (NFD → NFC)
  wrap.rs            표시 폭 기준 줄바꿈 (CJK 고려)
  render/mod.rs      pulldown-cmark 이벤트 → 스타일 줄 (블록 접두 스택, 목록/인용/각주 처리)
  render/code.rs     syntect 하이라이트
  render/table.rs    표 레이아웃
  render/canvas.rs   문자 격자 캔버스 (박스 문자 이음, 전각 문자 처리)
  render/html_table.rs  HTML <table> (rowspan/colspan 병합)
  render/math/       LaTeX 수식: layout.rs(2차원 상자 모델) · symbols.rs(기호 표) · mod.rs(파서)
  render/mermaid/    mermaid: graph.rs(계층 배치) · flow/state/er/class(그래프 계열 파서)
                     · sequence · gantt · pie · git · parse.rs(공통 도우미)
  output/ansi.rs     Line → ANSI 문자열
  output/tui_convert.rs  Line → ratatui 텍스트
  output/pager.rs    페이저 TUI
  output/picker.rs   파일 브라우저 TUI (Local / Stashed 탭, 트리·평면 보기, 미리보기 창)
  output/tree.rs     파일 목록 → 계층 행 펼치기 (순수 함수, 접힘 처리)
  output/watch.rs    목록 자동 갱신: 백그라운드 재검사, 스냅숏 비교(추가·변경·삭제)
packaging/
  arch/PKGBUILD          Arch Linux 패키지
  debian/build-deb.sh    Debian/Ubuntu .deb 빌드 스크립트
install.sh               배포판 자동 감지 설치 스크립트
examples/sample.md       기능 확인용 샘플 문서
examples/markdown-sample.md  수식·도표·다이어그램 종합 샘플
docs/superpowers/specs/  최초 설계 문서
```

데이터 흐름: 마크다운 문자열 → `render::render(md, &Theme, width)` → `Vec<Line>` → ANSI 출력 또는 ratatui 페이저.

## 4. 개발 과정 요약

1. 설계 문서 작성, cargo 프로젝트 생성, 의존성 추가
2. 문서 모델·테마·줄바꿈 모듈을 테스트와 함께 구현 (한글 폭 계산 확인)
3. 렌더러, 코드 하이라이트, 표 모듈 구현 (H1 블록, 각주 캡처, 느슨한 목록 빈 줄 처리)
4. ANSI 출력기, 페이저, 파일 브라우저, CLI 구현. expect로 가상 터미널 자동 조작 검증
5. `cargo install --path .` 설치 후 `~/.cargo/bin`을 `~/.zshrc`의 PATH에 추가
6. 스태시(로컬 즐겨찾기)와 원격 URL 읽기 추가. GitHub README/blob 변환, 오류 처리 확인
7. Arch Linux 호환성 검토(시스템 라이브러리 의존 없음, `ring`만 C 컴파일러 필요) 후 PKGBUILD와 MIT LICENSE 추가
8. Ubuntu/Debian용 `.deb` 빌드 스크립트 추가. Homebrew dpkg로 패키지 구조 검증
9. 수식·도표·다이어그램 지원 추가
   - 문자 격자 캔버스(`render/canvas.rs`)를 먼저 만들고 그 위에 모든 그림을 그리도록 정리
   - LaTeX 조판기: 기준선을 맞춰 상자를 잇는 2차원 모델 + 재귀 하강 파서
   - HTML 표 병합: 격자 배치 후 칸마다 테두리를 그려 이음 문자로 합치는 방식
   - mermaid 8종. 그래프 계열은 Sugiyama 방식을 단순화한 공통 배치기를 공유
   - 예제 문서를 40~400 폭으로 렌더링하는 통합 테스트와, 잘린 입력에도 패닉이 없는지 보는 테스트 추가
10. 사용성 수정
   - 제목에서 `#` 접두를 없애고 밑줄 방식으로 렌더링
   - macOS 자소 분리 한글을 읽을 때 합치도록 `hangul.rs` 추가
   - 색 구성을 무채색 + 푸른 계열 강조로 정리(코드 강조와 TUI 크롬 포함)
   - 터미널 출력이면 페이저를 기본으로 열고, 기존 직접 출력은 `-P/--print`로 이동
11. 파일 브라우저 보강
   - 오른쪽 미리보기 창(`p`). 목록 위젯은 평면 배열만 그리므로, 트리를 미리 행으로 펼쳐 두는
     `output/tree.rs`를 순수 함수로 분리해 단위 테스트를 붙였다
   - vi 이동 키(`j`/`k`, `Ctrl-d`/`u`/`f`/`b`, `gg`, `G`)
   - 디렉터리 계층 보기와 접기·펴기, `v`로 평면 보기와 전환
12. 미리보기 창 안에서 문서 넘겨보기 (0.3.1)
   - `l`로 화면 구성을 유지한 채 오른쪽 창에 포커스를 넘기고, 같은 이동 키로 스크롤
   - 포커스 위치를 선택 표시(`▌`/`│`)와 경계선 색, 푸터 백분율로 드러냄
   - `h`/`l`이 창 이동을 맡게 되어 트리 접기·펴기는 방향키·`Enter`로 정리
13. 트리 접기·펴기와 미리보기 폭 조절 (0.3.2)
   - `h`/`l`을 트리 접기·펴기로 되돌림. 접힌 디렉터리·파일 위의 `h`는 상위로, 열린 디렉터리의
     `l`은 첫 하위로, 파일 위의 `l`은 미리보기 창으로 (한 키가 문맥에 따라 자연스럽게 이어짐)
   - `H`/`L`로 모든 디렉터리 접기·펴기. 커서는 보던 항목(또는 그 최상위 디렉터리)에 남긴다
   - 마우스: 구분선을 끌어 미리보기 폭 조절(양쪽 최소 폭 20/30열 보장), 휠 스크롤, 클릭 선택·포커스
14. 상위 폴더 이동과 자동 갱신 (0.3.3)
   - `../` 행·`Backspace`·`-`로 상위 폴더. 넓은 폴더는 백그라운드에서 훑는다
   - 폴링 감시 스레드로 목록·미리보기·페이저가 파일 추가·삭제·수정을 따라간다
   - 변경 표시(`● 새 파일`/`● 변경됨`)와 푸터 요약, 스크롤 위치 유지
   - 작업 전에 GitHub 최신(0.3.2, 다른 세션에서 추가한 h/l 접기·H/L 전체·마우스 폭 조절)을 받아 그 위에서 진행
15. 경로 입력으로 폴더 이동 (0.3.4)
   - `c`로 `cd:` 입력창. 절대·`~`·상대 경로, `Tab` 자동 완성, `Ctrl-u`로 지우기, `Esc` 취소
   - 상위 이동과 루트 전환 코드를 `set_root`로 합침. 잘못된 경로는 입력창을 유지한 채 이유만 표시
   - 상위 폴더 이동 체감 속도 개선: 이전 목록을 즉시 보여 주고, 훑는 결과를 부분 스냅숏으로 흘려보내며,
     연타 시 이전 루트 훑기를 취소. 전에는 전체 재귀 훑기가 끝날 때까지 빈 화면에 `훑는 중…`만 보였다

## 5. 설치

### 공통 준비물
- **Rust 1.88 이상** (`cargo`, `rustc`). let-chains(`if let ... && ...`)를 쓰므로 그 아래 버전에서는 빌드되지 않는다.
  배포판 패키지가 오래되었으면 [rustup](https://rustup.rs) 사용
- C 컴파일러 (gcc 또는 clang). TLS 라이브러리 `ring` 빌드에 필요
- 그 밖의 시스템 라이브러리는 필요 없음 (OpenSSL 불필요)

### 자동 설치 스크립트
zip을 풀고 저장소 루트에서 실행하면 배포판을 감지해 알맞은 방식으로 설치합니다.

```sh
unzip mdview-0.3.4.zip && cd mdview
./install.sh
```

- Arch Linux: `makepkg -si` (pacman 패키지로 설치, `sudo pacman -R mdview`로 제거)
- Debian/Ubuntu: `.deb` 생성 후 `sudo apt install ./dist/mdview_*.deb` (`sudo apt remove mdview`로 제거)
- 그 외: `cargo install --path .` (`~/.cargo/bin/mdview`)

### Arch Linux 수동 설치

```sh
sudo pacman -S --needed base-devel rust
cd packaging/arch && makepkg -si
```

### Ubuntu / Debian 수동 설치

```sh
sudo apt install build-essential curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
./packaging/debian/build-deb.sh
sudo apt install ./dist/mdview_0.3.4_amd64.deb   # arm64 등 아키텍처에 따라 다름
```

### macOS 또는 cargo만 사용

```sh
cargo install --path .
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc   # bash면 ~/.bashrc
```

## 6. 사용법

```sh
mdview README.md                      # 스타일 출력
mdview -p README.md                   # 페이저
cat README.md | mdview                # 표준입력
mdview                                # 파일 브라우저
mdview docs/                          # 특정 디렉터리 브라우저
mdview github.com/charmbracelet/glow  # GitHub README
mdview https://example.com/doc.md     # 임의 URL
mdview -s light -w 80 a.md            # 밝은 테마, 폭 80
mdview --no-color a.md                # 색 없이
mdview stash a.md -n "메모"           # 스태시 저장
mdview stash                          # 스태시 목록
mdview stash -r a.md                  # 스태시 제거
```

### 페이저 키

| 키 | 동작 |
|----|------|
| `j` / `k`, 방향키, 마우스 휠 | 한 줄 이동 |
| `Space`, `PgDn` / `PgUp`, `Ctrl-f` / `Ctrl-b` | 한 화면 이동 |
| `Ctrl-d` / `Ctrl-u` | 반 화면 이동 |
| `g` / `G` | 처음 / 끝 |
| `/` | 검색, `n` / `N` 다음 / 이전, `Esc` 해제 |
| `s` | 스태시에 저장 |
| `Esc`, `Backspace` | 브라우저로 돌아가기 (브라우저에서 연 경우) |
| `q` | 종료 |

### 파일 브라우저 키

| 키 | 동작 |
|----|------|
| `j` / `k`, 방향키 | 한 줄 이동 |
| `Ctrl-d` / `Ctrl-u` | 반 페이지 |
| `Ctrl-f` / `Ctrl-b`, `PgDn` / `PgUp` | 한 페이지 |
| `gg` / `G`, `Home` / `End` | 처음 / 끝 |
| `Enter` | 파일이면 열기, 디렉터리면 접기·펴기 |
| `h`, `←` | 디렉터리 접기 (접힌 디렉터리·파일 위에서는 상위로) |
| `l`, `→` | 디렉터리 펴기 (열린 디렉터리면 첫 하위로), 파일 위에서는 미리보기 창으로 |
| `H` / `L` | 모든 디렉터리 접기 / 펴기 |
| 마우스 | 구분선 끌기로 미리보기 폭 조절, 휠 스크롤, 클릭 선택 |
| `Backspace`, `-` | 상위 폴더로 (`../` 행에서 `Enter`도 같음) |
| `v` | 트리 ↔ 평면 보기 |
| `p` | 미리보기 켜기 / 끄기 |

미리보기 창 안에서(`l`로 진입):

| 키 | 동작 |
|----|------|
| `j` / `k`, `Ctrl-d`/`Ctrl-u`, `Ctrl-f`/`Ctrl-b`, `Space` | 스크롤 |
| `gg` / `G` | 문서 처음 / 끝 |
| `h`, `Esc`, `←` | 목록으로 |
| `Enter` | 전체 화면 페이저로 열기 |
| `/` | 이름 필터 |
| `Tab`, `1` / `2` | Local ↔ Stashed 탭 |
| `s` | (Local) 스태시 저장 |
| `x` / `m` | (Stashed) 삭제 / 메모 편집 |
| `q` | 종료 |

## 7. 검증 내역

### 0.1.0 (macOS에서 개발)
- 단위 테스트 51개: 줄바꿈(ASCII/한글/일본어/긴 단어/스타일 유지), 렌더러(각 마크다운 요소), 표, 구문 강조, ANSI 변환, 파일 탐색, URL 판별과 GitHub 변환, 스태시 저장/중복/삭제/메모
- expect 가상 터미널: 페이저 이동·검색·종료, 브라우저 필터·열기·복귀, 스태시 저장→탭 전환→메모→열기→중복 안내→삭제
- 실제 네트워크: glow README, rust 저장소 CONTRIBUTING.md 렌더링, 404와 없는 호스트 오류 처리
- PKGBUILD: macOS에서 prepare/build/check/package 단계를 직접 실행해 산출물 확인
- .deb: `dpkg-deb --info/--contents`로 제어 파일과 파일 배치(root 소유) 확인, 추출한 바이너리 실행 확인

### 0.2.0 / 0.3.0 (Arch Linux에서 검증)
- 테스트 173개 통과, `cargo clippy --all-targets` 경고 없음
- 통합 테스트: 예제 문서 전체를 폭 40~400으로 렌더링해 줄 넘침이 없는지 확인,
  잘린 mermaid·LaTeX 입력 40여 가지에 패닉이 없는지 확인
- pty(가상 터미널)로 실제 화면을 재구성해 확인: 페이저가 기본으로 열리는지,
  파일 브라우저 Local·Stashed 목록에 자소 분리가 남지 않는지,
  브라우저의 트리 보기·미리보기·vi 이동 키(`gg`는 두 번 눌러야 동작)가 실제로 먹는지
- `makepkg -sif`로 Arch 패키지를 빌드해 `/usr/bin/mdview`에 설치, 설치본으로 기능 재확인
  (빌드 중 `cargo test --frozen --release`가 함께 돈다)

## 8. 배포 점검 (macOS / Ubuntu)

0.2.0 기준으로 점검한 결과입니다. 점검은 Arch Linux에서 수행했으므로, macOS·Ubuntu 항목은
소스·의존성·패키징 절차를 따져 본 것이며 실제 그 OS에서 빌드해 본 것은 아닙니다(아래 "남은 확인" 참고).

### 소스 이식성
- OS 분기 코드 없음: `cfg(target_os)`, `cfg(unix)`, `std::os::*` 사용처 0곳
- 경로를 문자열로 짜맞추는 곳 없음. 모두 `Path`/`PathBuf` 사용
- 파일 확장자 비교는 대소문자 무시(`eq_ignore_ascii_case`) → 대소문자를 구분하지 않는 APFS에서도 동작
- 환경변수는 `HOME`, `XDG_CONFIG_HOME`, `MDVIEW_STASH`, `MDVIEW_STYLE`, `NO_COLOR`, `COLORFGBG` 뿐.
  모두 macOS·Linux 공통
- 임시 경로는 `std::env::temp_dir()` 사용

### 의존성
네 타깃 모두 의존성 그래프가 해결됩니다(`cargo tree --target`).

| 타깃 | 결과 |
|------|------|
| `x86_64-apple-darwin` | 해결됨 |
| `aarch64-apple-darwin` (Apple Silicon) | 해결됨 |
| `x86_64-unknown-linux-gnu` | 해결됨 |
| `aarch64-unknown-linux-gnu` | 해결됨 |

macOS와 Linux의 의존성 차이는 `rustix`가 고르는 백엔드 한 개뿐입니다(macOS `errno` ↔ Linux `linux-raw-sys`).
OS 전용 크레이트(winapi, core-foundation, objc 등)는 들어오지 않고, C 코드는 rustls의 `ring`만 쓰므로
C 컴파일러(gcc 또는 clang)만 있으면 됩니다. OpenSSL 같은 시스템 라이브러리는 필요 없습니다.

### Rust 버전 — 주의
let-chains(`if let ... && ...`)를 쓰므로 **rustc 1.88 이상**이 필요합니다.
`Cargo.toml`에 `rust-version = "1.88"`을 명시해 두었으므로, 낮은 버전에서는 문법 오류 대신
"requires rustc 1.88" 안내가 나옵니다.

Ubuntu의 apt `rustc`는 24.04가 1.75, 25.04도 1.8x대라 **어느 쪽도 부족합니다.** rustup을 쓰세요.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Ubuntu / Debian
- `.deb` 빌드 스크립트를 `dpkg-deb`만 대역으로 바꿔 모의 실행해, 0.2.0 버전이 반영되고
  control 파일과 파일 배치가 맞는지 확인했습니다.
- `Depends: libc6, libgcc-s1`은 실제 링크와 일치합니다(`ldd` 결과 libc, libm, libgcc_s만 참조).
- 아키텍처는 `dpkg --print-architecture`, 없으면 `uname -m`으로 정합니다(amd64 / arm64 / armhf).

### macOS
- `install.sh`는 `/etc/os-release`가 없으면 `cargo` 모드로 떨어집니다. macOS에서는 `cargo install --path .`가 쓰입니다.
- Homebrew 포뮬러는 제공하지 않습니다. `cargo install --path .` 또는 `cargo build --release` 후 바이너리 복사를 쓰세요.
- **배경색 자동 판별이 안 됩니다.** Terminal.app과 iTerm2는 `COLORFGBG`를 내보내지 않아
  `-s auto`가 무조건 dark를 고릅니다. 밝은 배경을 쓴다면 셸 설정에 한 줄 넣으세요.

  ```sh
  echo 'export MDVIEW_STYLE=light' >> ~/.zshrc
  ```

  `MDVIEW_STYLE`은 `dark|light|notty|auto`를 받고, `-s` 옵션이 있으면 그쪽이 이깁니다.
- 글리프: 수식의 늘어난 괄호(`⎡⎢⎣`, `⎧⎨⎩`)와 다이어그램의 블록 문자(`▒▓█`)를 씁니다.
  Menlo·SF Mono·JetBrains Mono 등에는 있지만, 폰트에 없으면 대체 폰트로 떨어지면서 폭이 어긋날 수 있습니다.
  그럴 때는 터미널 폰트를 바꾸세요.

### 남은 확인
실제 macOS와 Ubuntu 머신에서 `./install.sh`를 끝까지 돌려 본 것은 아닙니다.
각 OS에서 다음 한 줄이면 검증됩니다.

```sh
cargo test && ./install.sh && mdview --version && mdview examples/markdown-sample.md
```

## 9. 알려진 제한과 다음 단계

- glow의 클라우드 스태시(Charm 계정 동기화)는 구현하지 않았습니다. 로컬 JSON 파일만 사용합니다.
- 페이저는 문서 전체를 메모리에 렌더링합니다. 수만 줄 문서도 문제없지만 매우 큰 파일은 느릴 수 있습니다.
- 이미지는 대체 텍스트와 URL만 표시합니다.
- 배경색 자동 판별은 `COLORFGBG`에만 의존합니다. OSC 11로 터미널에 직접 물어보는 방식이 더 정확하지만,
  응답하지 않는 터미널에서 멈추지 않게 만들어야 해서 지금은 `MDVIEW_STYLE` 수동 지정으로 두었습니다.
- mermaid는 8종을 지원합니다. `mindmap`, `journey`, `quadrantChart` 등은 원문 코드블록으로 보여 줍니다.
- 다이어그램은 방향 지정(`LR`/`TB`)과 무관하게 항상 위에서 아래로 배치합니다.
