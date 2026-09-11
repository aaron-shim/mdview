use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum StyleArg {
    Auto,
    Dark,
    Light,
    Notty,
}

/// Render markdown on the CLI, with pizzazz.
#[derive(Parser, Debug)]
#[command(name = "mdview", version, about, long_about = None, args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Markdown file, directory, or URL to render. Use "-" for stdin. Omit to browse the current directory.
    pub source: Option<String>,

    /// Open in the scrollable pager (default when stdout is a terminal).
    #[arg(short, long)]
    pub pager: bool,

    /// Print the rendered document straight to stdout instead of opening the pager.
    #[arg(short = 'P', long, visible_alias = "no-pager", conflicts_with = "pager")]
    pub print: bool,

    /// Color style: auto, dark, light, or notty.
    #[arg(short, long, value_enum, default_value_t = StyleArg::Auto, global = true)]
    pub style: StyleArg,

    /// Word-wrap width (0 = terminal width, capped at 120).
    #[arg(short, long, default_value_t = 0, global = true)]
    pub width: usize,

    /// Disable colors entirely (same as --style notty).
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Save a file or URL to the local stash (favorites).
    Stash(StashArgs),
}

#[derive(Args, Debug)]
pub struct StashArgs {
    /// File path or URL to stash.
    pub source: Option<String>,

    /// Optional note to attach.
    #[arg(short, long)]
    pub note: Option<String>,

    /// List stashed documents.
    #[arg(short, long)]
    pub list: bool,

    /// Remove the given file or URL from the stash.
    #[arg(short, long)]
    pub remove: bool,
}
