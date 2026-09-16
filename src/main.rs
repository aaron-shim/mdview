mod cli;
mod doc;
mod hangul;
mod output;
mod render;
mod source;
mod stash;
mod theme;
mod wrap;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Command, StashArgs, StyleArg};
use output::pager::{Pager, PagerExit};
use output::picker::{Picker, PickerExit};
use source::{Document, Source};
use stash::Stash;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use theme::Theme;

const MAX_WIDTH: usize = 120;

fn main() {
    if let Err(e) = run() {
        eprintln!("mdview: {e:#}");
        std::process::exit(1);
    }
}

fn pick_theme(cli: &Cli, tty_out: bool) -> Theme {
    if cli.no_color || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return Theme::notty();
    }
    // `-s`를 주지 않았으면 MDVIEW_STYLE 환경변수를 본다.
    let style = match cli.style {
        StyleArg::Auto => env_style().unwrap_or(StyleArg::Auto),
        explicit => explicit,
    };
    match style {
        StyleArg::Dark => Theme::dark(),
        StyleArg::Light => Theme::light(),
        StyleArg::Notty => Theme::notty(),
        StyleArg::Auto => {
            if !tty_out {
                Theme::notty()
            } else if looks_light_background() {
                Theme::light()
            } else {
                Theme::dark()
            }
        }
    }
}

/// `MDVIEW_STYLE=dark|light|notty|auto`.
///
/// macOS 터미널(Terminal.app, iTerm2)은 `COLORFGBG`를 내보내지 않아서 밝은 배경을
/// 자동으로 알아낼 수 없다. 셸 설정에 이 값을 한 번 넣어 두면 해결된다.
fn env_style() -> Option<StyleArg> {
    parse_style(&std::env::var("MDVIEW_STYLE").ok()?)
}

fn parse_style(v: &str) -> Option<StyleArg> {
    match v.trim().to_ascii_lowercase().as_str() {
        "dark" => Some(StyleArg::Dark),
        "light" => Some(StyleArg::Light),
        "notty" | "none" | "no-color" => Some(StyleArg::Notty),
        "auto" => Some(StyleArg::Auto),
        _ => None,
    }
}

/// COLORFGBG 환경변수("fg;bg")로 밝은 배경을 추정한다.
/// 이 값을 내보내는 터미널은 많지 않다(rxvt, konsole 등).
fn looks_light_background() -> bool {
    std::env::var("COLORFGBG")
        .ok()
        .and_then(|v| v.rsplit(';').next().and_then(|bg| bg.parse::<u8>().ok()))
        .is_some_and(|bg| bg == 7 || bg == 15)
}

/// 페이저로 열지 여부. 터미널 출력이면 기본으로 연다.
fn use_pager(cli: &Cli, tty_out: bool) -> bool {
    tty_out && (cli.pager || !cli.print)
}

fn terminal_width() -> usize {
    crossterm::terminal::size().ok().map(|(w, _)| w as usize).filter(|w| *w > 0).unwrap_or(80)
}

fn content_width(cli: &Cli) -> usize {
    if cli.width > 0 { cli.width } else { terminal_width().min(MAX_WIDTH) }
}

/// 명령줄 인자를 소스로 해석한다. 디렉터리는 None (브라우저로 처리).
fn parse_source_arg(arg: &str) -> Source {
    if arg == "-" {
        Source::Stdin
    } else if Path::new(arg).exists() {
        Source::File(PathBuf::from(arg))
    } else if source::looks_like_url(arg) {
        let url = if source::is_url(arg) { arg.to_string() } else { format!("https://{arg}") };
        Source::Url(url)
    } else {
        Source::File(PathBuf::from(arg))
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Command::Stash(args)) = &cli.command {
        return stash_command(args);
    }
    let tty_out = source::stdout_is_tty();
    let theme = pick_theme(&cli, tty_out);
    let stash = Rc::new(RefCell::new(Stash::load()));

    // 입력 결정
    let src: Option<Source> = match &cli.source {
        Some(arg) if Path::new(arg).is_dir() => return browse(&cli, theme, PathBuf::from(arg), stash),
        Some(arg) => Some(parse_source_arg(arg)),
        None if !source::stdin_is_tty() => Some(Source::Stdin),
        None => None,
    };

    let Some(src) = src else {
        // 인자도 stdin도 없음 → 파일 브라우저
        let cwd = std::env::current_dir().context("cannot determine current directory")?;
        return browse(&cli, theme, cwd, stash);
    };

    let doc = source::load(&src)?;
    // 터미널로 출력할 때는 페이저가 기본. `--print`를 주거나 파이프면 그대로 찍는다.
    if use_pager(&cli, tty_out) {
        let mut terminal = init_terminal();
        let result = open_pager(&cli, &theme, doc, false, stash, &mut terminal);
        restore_terminal();
        result?;
        return Ok(());
    }
    print_document(&cli, &theme, &doc)
}

/// `mdview stash ...` 서브커맨드.
fn stash_command(args: &StashArgs) -> Result<()> {
    let mut stash = Stash::load();
    if args.list || (args.source.is_none() && !args.remove) {
        if stash.entries.is_empty() {
            println!("stash is empty ({})", stash.path().display());
            return Ok(());
        }
        for e in &stash.entries {
            match &e.note {
                Some(n) => println!("{}\t{}", e.source, n),
                None => println!("{}", e.source),
            }
        }
        return Ok(());
    }
    let Some(arg) = &args.source else { bail!("stash --remove needs a file or URL") };
    let src = parse_source_arg(arg);
    if let Source::File(p) = &src
        && !p.is_file() {
            bail!("{} is not a file", p.display());
        }
    let Some(key) = src.stash_key() else { bail!("cannot stash stdin") };
    if args.remove {
        match stash.find(&key) {
            Some(i) => {
                stash.remove(i);
                stash.save()?;
                println!("removed {key}");
            }
            None => bail!("{key} is not in the stash"),
        }
        return Ok(());
    }
    let added = stash.add(&key, args.note.clone());
    stash.save()?;
    println!("{} {key}", if added { "stashed" } else { "already stashed, updated" });
    Ok(())
}

fn print_document(cli: &Cli, theme: &Theme, doc: &Document) -> Result<()> {
    let width = content_width(cli);
    let lines = render::render(&doc.text, theme, width);
    let mut s = output::ansi::to_string(&lines);
    if !s.is_empty() {
        s.insert(0, '\n');
    }
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    // 파이프가 닫혀도 조용히 종료
    if let Err(e) = lock.write_all(s.as_bytes()).and_then(|_| lock.flush()) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    Ok(())
}

fn init_terminal() -> ratatui::DefaultTerminal {
    let terminal = ratatui::init();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    terminal
}

fn restore_terminal() {
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    ratatui::restore();
}

/// 문서를 페이저로 연다. 터미널은 호출자가 초기화/복원한다.
fn open_pager(
    cli: &Cli,
    theme: &Theme,
    doc: Document,
    from_picker: bool,
    stash: Rc<RefCell<Stash>>,
    terminal: &mut ratatui::DefaultTerminal,
) -> Result<PagerExit> {
    // 파일 문서는 편집기에서 고치는 동안에도 따라가도록 내용을 공유해 둔다.
    let text = Rc::new(RefCell::new(doc.text));
    let reload = match &doc.source {
        Source::File(p) => Some(file_reloader(p.clone(), text.clone())),
        _ => None,
    };
    let theme = theme.clone();
    let fixed_width = cli.width;
    let rerender = Box::new(move |term_w: usize| {
        let w = if fixed_width > 0 { fixed_width.min(term_w) } else { term_w.min(MAX_WIDTH) };
        render::render(&text.borrow(), &theme, w)
    });
    let mut pager = Pager::new(doc.title, terminal_width(), rerender, from_picker, stash, doc.source.stash_key(), reload);
    pager.run(terminal)
}

/// 파일이 바뀌었는지 살피는 콜백. 바뀌었으면 `text`를 새 내용으로 채우고 true.
///
/// 편집기가 저장하며 잠깐 파일을 지웠다가 다시 만드는 경우가 있어,
/// 파일이 안 보이는 순간에는 기다렸다가 다음 확인에서 읽는다.
fn file_reloader(path: PathBuf, text: Rc<RefCell<String>>) -> Box<dyn FnMut() -> bool> {
    let mut last = source::stamp_of(&path);
    Box::new(move || {
        let now = source::stamp_of(&path);
        if now.is_none() || now == last {
            return false;
        }
        let Ok(bytes) = std::fs::read(&path) else { return false };
        last = now;
        *text.borrow_mut() = hangul::compose(&String::from_utf8_lossy(&bytes));
        true
    })
}

fn browse(cli: &Cli, theme: Theme, root: PathBuf, stash: Rc<RefCell<Stash>>) -> Result<()> {
    if !source::stdout_is_tty() {
        bail!("no input given and stdout is not a terminal (try: mdview FILE.md)");
    }
    let root = root.canonicalize().unwrap_or(root);
    let scan = source::scan_markdown_files(&root, cli.hidden);
    let files = scan.iter().map(|(p, _)| p.clone()).collect();
    let mut picker = Picker::new(root, files, stash.clone(), theme.clone(), cli.hidden);
    // 파일이 생기거나 바뀌면 목록과 미리보기를 자동으로 갱신한다.
    picker.start_watching(scan);
    // 페이저와 브라우저를 오갈 때 터미널을 한 번만 초기화한다.
    let mut terminal = init_terminal();
    let result = browse_loop(cli, &theme, &mut picker, stash, &mut terminal);
    restore_terminal();
    result
}

fn browse_loop(cli: &Cli, theme: &Theme, picker: &mut Picker, stash: Rc<RefCell<Stash>>, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    loop {
        match picker.run(terminal)? {
            PickerExit::Quit => return Ok(()),
            PickerExit::Open(src, title) => {
                let doc = match source::load(&src) {
                    Ok(d) => Document { title, ..d },
                    Err(e) => Document { title, text: format!("# Error\n\n{e:#}"), source: src },
                };
                match open_pager(cli, theme, doc, true, stash.clone(), terminal)? {
                    PagerExit::Quit => return Ok(()),
                    PagerExit::Back => continue,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_reloader_notices_edits_once() {
        let dir = std::env::temp_dir().join(format!("mdview-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "# 처음").unwrap();
        let text = Rc::new(RefCell::new(String::from("# 처음")));
        let mut reload = file_reloader(file.clone(), text.clone());
        assert!(!reload(), "그대로면 다시 읽지 않는다");
        std::fs::write(&file, "# 고친 내용입니다").unwrap();
        assert!(reload(), "바뀌면 다시 읽는다");
        assert_eq!(*text.borrow(), "# 고친 내용입니다");
        assert!(!reload(), "한 번 읽은 변경은 다시 알리지 않는다");
        std::fs::remove_file(&file).unwrap();
        assert!(!reload(), "잠깐 사라진 동안에는 기다린다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn style_env_values_parse() {
        assert_eq!(parse_style("light"), Some(StyleArg::Light));
        assert_eq!(parse_style(" DARK "), Some(StyleArg::Dark));
        assert_eq!(parse_style("notty"), Some(StyleArg::Notty));
        assert_eq!(parse_style("auto"), Some(StyleArg::Auto));
        assert_eq!(parse_style("파랑"), None, "모르는 값은 무시하고 자동 판별로 돌아간다");
        assert_eq!(parse_style(""), None);
    }
}
