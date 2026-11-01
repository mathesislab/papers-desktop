//! The command line: the commands, their arguments and their help.

use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};
use papers_feeds::{ItemKind, Sort, SourceKind, Triage};

/// The new papers of journals, preprint servers, news sites and blogs: what paper-feeder did, and more.
#[derive(Debug, Parser)]
#[command(name = "feeds", version, propagate_version = true)]
pub struct Cli {
    /// The folder of the feeds [env: FEEDS_DIR; default: papers-feeds/ in the data folder of the
    /// platform]. Do not point it at the folder of the app while the app runs: nothing locks it
    #[arg(long, global = true, value_name = "FOLDER")]
    pub dir: Option<PathBuf>,
    /// A .bib file of the library (repeated for several): `like:`, `--sort like`, and the
    /// nearest papers of `show` tell how like its papers an item is
    #[arg(long, global = true, value_name = "FILE")]
    pub library: Vec<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Follow a feed (a URL), a journal's Crossref sweep (--issn) or an OpenAlex filter (--openalex)
    Subscribe(SubscribeArgs),
    /// Change a subscription
    Edit(EditArgs),
    /// Stop following a subscription (its items stay until pruned)
    Unsubscribe(IdArg),
    /// The subscriptions
    Subscriptions(JsonArg),
    /// Fetch every subscription, then the abstracts and the papers behind posts
    Fetch(AccessArgs),
    /// The items, one a line: id, state, score, kind, date, title, journal
    ///
    /// The state column: `*` starred, `x` removed, `+` added, `.` seen, blank new.
    /// Removed items are left out unless asked for.
    List(ListArgs),
    /// All that is known of an item
    Show(ShowArgs),
    /// Look again for the papers behind a news item or a blog post (needs the network)
    Find(FindArgs),
    /// Set the state of items
    Mark(MarkArgs),
    /// Drop the old items no feed lists (starred and added stay); done on opening too
    Prune,
    /// The interests: weighted terms that score the items
    Interests(JsonArg),
    /// The rules that triage new items
    Rules(JsonArg),
    /// The saved views (as the Feeds tab's sidebar): name, order, query; or one added or removed
    #[command(alias = "searches")]
    Views(ViewsArgs),
    /// The folder and the settings; `keep-days` and `lookback-days` change one
    Config(ConfigArgs),
    /// The subscriptions as OPML, the list feed readers exchange
    Opml(OpmlArgs),
    /// Write the items of the last days that reach an interest as an RSS feed or an HTML page
    #[command(group(ArgGroup::new("output").required(true).multiple(true).args(["rss", "html"])))]
    Digest(DigestArgs),
}

/// What the APIs of Crossref and OpenAlex are asked with.
#[derive(Debug, Args)]
pub struct AccessArgs {
    /// An address for the polite pools of Crossref and OpenAlex [env: PAPERS_MAILTO]
    #[arg(long, value_name = "ADDRESS")]
    pub mailto: Option<String>,
    /// OpenAlex's key, which raises its small daily budget without one [env: OPENALEX_API_KEY]
    #[arg(long, value_name = "KEY")]
    pub openalex_key: Option<String>,
}

/// An id.
#[derive(Debug, Args)]
pub struct IdArg {
    pub id: String,
}

/// The choice of JSON.
#[derive(Debug, Args)]
pub struct JsonArg {
    /// As JSON
    #[arg(long)]
    pub json: bool,
}

/// `feeds config`.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// A setting to change; none: show them all
    #[command(subcommand)]
    pub set: Option<ConfigSet>,
}

/// The settings `feeds config` changes.
#[derive(Debug, Subcommand)]
pub enum ConfigSet {
    /// The days an item no feed lists is kept, unless starred or added
    KeepDays {
        #[arg(value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
        days: u32,
    },
    /// The days a sweep of Crossref or OpenAlex looks back the first time
    LookbackDays {
        #[arg(value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
        days: u32,
    },
}

/// `feeds opml`.
#[derive(Debug, Args)]
pub struct OpmlArgs {
    #[command(subcommand)]
    pub command: OpmlCommand,
}

/// What `feeds opml` does.
#[derive(Debug, Subcommand)]
pub enum OpmlCommand {
    /// Follow the feeds and sweeps of an OPML file (those followed already are passed over)
    Import { file: PathBuf },
    /// Write the subscriptions as OPML to a file, else to the standard output
    Export { file: Option<PathBuf> },
}

/// `feeds subscribe`.
#[derive(Debug, Args)]
#[command(group(ArgGroup::new("feed").required(true).args(["url", "issn", "openalex"])))]
pub struct SubscribeArgs {
    /// The address of an RSS or Atom feed
    pub url: Option<String>,
    /// Sweep a journal's new works in Crossref by its ISSN
    #[arg(long, value_name = "ISSN")]
    pub issn: Option<String>,
    /// Sweep the new works of an OpenAlex filter, e.g. `primary_topic.id:T10644` (needs a key)
    #[arg(long, value_name = "FILTER")]
    pub openalex: Option<String>,
    /// What its items are: journal, preprints, news or blog [default: preprints for arXiv and the
    /// preprint servers, else journal]
    #[arg(long)]
    pub kind: Option<SourceKind>,
    /// A title of its own, over the feed's
    #[arg(long)]
    pub title: Option<String>,
    /// The journal of its items, over what the feed says
    #[arg(long)]
    pub journal: Option<String>,
    /// Fetch the page of each item at fetch time (to find the paper of a post)
    #[arg(long)]
    pub fetch_pages: bool,
    /// Added to the score of its items, e.g. 2 for "cites my work"
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub boost: Option<f64>,
}

/// `feeds edit`.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// The id of the subscription
    pub id: String,
    /// Added to the score of its items in each interest (0 for none); the strongest of the
    /// subscriptions that list an item counts
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub boost: f64,
}

/// `feeds list`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// A query: words, `kind:`, `is:`, `src:`, `prof:`, `score:3-`, `like:30-`, `abs:`, ...
    pub query: Vec<String>,
    /// The items of a saved view (and of the query, if there is one)
    #[arg(long, value_name = "NAME")]
    pub saved: Option<String>,
    /// The order: score (the best first), newest, or like (the most like the library first, with
    /// --library) [default: the saved view's, else newest]
    #[arg(long)]
    pub sort: Option<Sort>,
    /// Only these kinds, separated by commas: paper, preprint, news, blog
    #[arg(long, value_delimiter = ',')]
    pub kind: Vec<ItemKind>,
    /// At most this many
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
    /// Removed items too
    #[arg(long)]
    pub removed: bool,
    /// As JSON
    #[arg(long)]
    pub json: bool,
}

/// `feeds show`.
#[derive(Debug, Args)]
pub struct ShowArgs {
    pub id: String,
    /// As JSON
    #[arg(long)]
    pub json: bool,
}

/// `feeds find`.
#[derive(Debug, Args)]
pub struct FindArgs {
    pub id: String,
    #[command(flatten)]
    pub access: AccessArgs,
}

/// `feeds mark`.
#[derive(Debug, Args)]
pub struct MarkArgs {
    /// new, seen, starred, removed or added
    pub state: Triage,
    #[arg(required = true)]
    pub ids: Vec<String>,
}

/// `feeds digest`.
#[derive(Debug, Args)]
pub struct DigestArgs {
    /// Write the RSS feed to this file
    #[arg(long, value_name = "FILE")]
    pub rss: Option<PathBuf>,
    /// Write the HTML page to this file
    #[arg(long, value_name = "FILE")]
    pub html: Option<PathBuf>,
    /// Which items: a query, best score first [default: score:3-, none with --saved]
    #[arg(long)]
    pub query: Option<String>,
    /// The items of a saved view (and of --query, if given)
    #[arg(long, value_name = "NAME")]
    pub saved: Option<String>,
    /// Only the items of the last days (by their date, else when first fetched)
    #[arg(long, default_value_t = 7, value_name = "N")]
    pub days: u32,
    /// The title of the feed and of the page
    #[arg(long, default_value = "Feeds")]
    pub title: String,
    /// The link of the feed: where the digest is published
    #[arg(long, default_value = "")]
    pub link: String,
}

/// `feeds views`.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ViewsArgs {
    #[command(subcommand)]
    pub action: Option<ViewAction>,
    /// As JSON
    #[arg(long)]
    pub json: bool,
}

/// What `feeds views` changes.
#[derive(Debug, Subcommand)]
pub enum ViewAction {
    /// Save a query under a name, in place of the view of that name (in any case)
    Add {
        name: String,
        /// The query, as `list` takes it (`is:new kind:paper prof:AMOC`)
        #[arg(required = true)]
        query: Vec<String>,
        /// The order of its items: score, newest or like
        #[arg(long, default_value = "newest")]
        sort: Sort,
    },
    /// Forget a saved view
    Remove { name: String },
}
