#![warn(missing_docs)]
#![warn(clippy::missing_docs_in_private_items)]

//! This crate provides a way to set or get ratings for songs based on listening statistics.
//! This is written for mpd as plugin. To work you have to have mpd running.
mod error;
mod listener;
mod stats;
use clap::{Parser, Subcommand};
use log::{debug, error, trace, warn};
use once_cell::sync::OnceCell;
use std::io::{Read, Write};
use std::path::PathBuf;

/// header name which will be used on either mpd's sticker database or tags for identifications
pub const MP_DESC: &str = "mpr";

/// defines connection type for the mpd.
#[derive(Debug)]
pub enum ConnType {
    /// connects through linux socket file
    Stream(std::os::unix::net::UnixStream),
    /// connects using normal network sockets
    Socket(std::net::TcpStream),
}

impl Read for ConnType {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ConnType::Stream(s) => s.read(buf),
            ConnType::Socket(s) => s.read(buf),
        }
    }
}

impl Write for ConnType {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            ConnType::Stream(s) => s.write(buf),
            ConnType::Socket(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            ConnType::Stream(s) => s.flush(),
            ConnType::Socket(s) => s.flush(),
        }
    }
}

/// contains root dir string optionally either if the user passes through cmdline or if the unix
/// socket file is given
static ROOT_DIR: OnceCell<PathBuf> = OnceCell::new();

/// Subcommands for config options
#[derive(Subcommand, Debug)]
enum Commands {
    /// listens for mpd events
    #[command()]
    Listen,
    /// extracts stats of given songs
    #[command()]
    GetStats(stats::GetStatsConfig),
    /// manually set stats for a perticular song, it should be in json
    #[command()]
    SetStats(stats::SetStatsConfig),
    /// export stats to a file
    #[command()]
    Export {
        /// output file[default it write to stdout]
        #[arg(short, long)]
        out_file: Option<PathBuf>,
        /// exports with songs hash. this way songs name is not required to be matching
        #[arg(short = 'H', long)]
        hash: bool,
    },
    /// import stats from a file
    #[command()]
    Import {
        /// strategy to import songs
        #[arg(value_enum, short='M', long, default_value_t=stats::ImportMethodConfig::Path)]
        method: stats::ImportMethodConfig,
        /// import stats and if there is already stats available then add both
        #[arg(short, long)]
        merge: bool,
        /// file containing stats, if not present then reads it from stdin
        #[arg()]
        input_file: Option<PathBuf>,
    },
    /// resets all stats to 0
    #[command()]
    Clear,
}

/// Arguments for mpr
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Config {
    /// Confirm to all prompts with y
    #[arg(short, long)]
    yes: bool,
    /// sets the verbose level, use multiple times for more verbosity. By default all the logs are written to stderr
    #[arg(short, long, action=clap::ArgAction::Count)]
    verbose: u8,
    /// use eyed3 tags to store ratings. If not specified by default mpd stickers are used. tags are persistante across file moves, where as incase of mpd sticker these will be erased if you move the files.
    #[arg(short = 't', long, env = "MPR_USE_TAGS")]
    use_tags: bool,
    /// path to mpd socket.
    /// if both path and socket address are specified, then path has higher priority.
    /// If  this flag is set then music directory is automatically taken from mpd"
    #[arg(short='p', long, default_value_t=format!("{}/.local/run/mpd/socket", std::env::var("HOME").unwrap_or_else(|_|".".to_string())))]
    socket_path: String,
    /// mpd's root directory
    #[arg(short, long)]
    root_dir: Option<std::path::PathBuf>,
    /// mpd socket address. <host>:<port> ex. -a 127.0.0.1:6600
    #[arg(short = 'a', long, default_value = "127.0.0.1:6600")]
    socket_address: String,
    /// subcommands for mpr
    #[command(subcommand)]
    command: Commands,
}

fn main() {
    let mut builder = env_logger::builder();
    let arguments = Config::parse();

    // set the verbosity
    match arguments.verbose {
        0 => builder.filter_module("mpr", log::LevelFilter::Error).init(),
        1 => builder.filter_module("mpr", log::LevelFilter::Warn).init(),
        2 => builder.filter_module("mpr", log::LevelFilter::Info).init(),
        3 => builder.filter_module("mpr", log::LevelFilter::Debug).init(),
        4 => builder.filter_module("mpr", log::LevelFilter::Trace).init(),
        _ => {
            builder.filter_level(log::LevelFilter::Trace).init();
            trace!("wait one of the rust expert is coming to debug");
        }
    }
    debug!("log_level set to {:?}", log::max_level());
    if arguments.use_tags {
        debug!("Using tags for storing stats");
    }
    let mut client = {
        debug!("trying to connect to unix stream {}", arguments.socket_path);
        std::os::unix::net::UnixStream::connect(arguments.socket_path).map_or_else(
            |err| {
                warn!("Failed to connect to unix stream due to {err}");
                debug!("connecting to TcpStream {}", arguments.socket_address);
                if arguments.use_tags {
                    if let Some(root_dir) = &arguments.root_dir {
                        debug!("Setting mpd root-dir to {:?}", root_dir);
                        ROOT_DIR.set(root_dir.to_path_buf()).unwrap();
                    } else {
                        error!(
                            "for socket connection if tags are required then root-dir must be set"
                        );
                        std::process::exit(1);
                    }
                }
                mpd::Client::new(ConnType::Socket(
                    std::net::TcpStream::connect(arguments.socket_address).unwrap(),
                ))
                .unwrap()
            },
            |conn| {
                let mut client = mpd::Client::new(ConnType::Stream(conn)).unwrap();
                ROOT_DIR
                    .set(PathBuf::from(client.music_directory().unwrap()))
                    .unwrap();
                client
            },
        )
    };
    match arguments.command {
        Commands::Listen => listener::listen(&mut client, arguments.use_tags),
        Commands::GetStats(config) => stats::get_stats(&mut client, &config, arguments.use_tags),
        Commands::SetStats(config) => stats::set_stats(&mut client, &config, arguments.use_tags),
        Commands::Import {
            method,
            merge,
            input_file,
        } => stats::import_stats(
            &mut client,
            method,
            input_file,
            merge,
            arguments.use_tags,
            arguments.yes,
        ),
        Commands::Export { out_file, hash } => {
            stats::export_stats(&mut client, out_file, hash, arguments.use_tags)
        }
        Commands::Clear => stats::clear_stats(&mut client, arguments.use_tags, arguments.yes),
    }
}
