#![warn(missing_docs)]
#![warn(clippy::missing_docs_in_private_items)]

//! This crate provides a way to set or get ratings for songs based on listening statistics.
//! This is written for mpd as plugin. To work you have to have mpd running.
mod error;
mod listener;
mod stats;
use clap::{Arg, Command};
use log::{debug, error, trace};
use once_cell::sync::OnceCell;
use std::io::{Read, Write};
use std::path::Path;
use std::process::exit;

/// header name which will be used on either mpd's sticker database or tags for identifications
pub const MP_DESC: &str = "mp_rater";

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
static ROOT_DIR: OnceCell<String> = OnceCell::new();

fn main() {
    let mut builder = env_logger::builder();
    let arguments = Command::new("mp rater")
        .version("0.1.0")
        .author("hardfau18 <the.qu1rky.b1t@gmail.com>")
        .about("rates song with skip/rate count for mpd")
        .arg(
            Arg::new("confirm")
            .short('y')
            .long("yes")
            .help("Confirm to all prompts")
            )
        .arg(
            Arg::new("verbose")
                .short('v')
                .global(true)
                .multiple_occurrences(true)
                .long("verbose")
                .help("sets the verbose level, use multiple times for more verbosity. By default all the logs are written to stderr")
        )
            .arg(
                Arg::new("use-tags")
                .short('t')
                .long("use-tags")
                .env("MP_RATER_USE_TAGS")
                .help("use eyed3 tags to store ratings. If not specified by default mpd stickers are used. tags are persistante across file moves, where as incase of mpd sticker these will be erased if you move the files. Else you can set MP_RATER_USE_TAGS=1 in environment variable")
                )
        .arg(Arg::new("socket-path")
         .short('p')
            .long("socket-path")
            .default_value(&format!("{}/.local/run/mpd/socket", std::env::var("HOME").unwrap_or_else(|_|".".to_string())))
            .takes_value(true)
            .help("path to mpd socket. \
                By default it will check in ~/.local/run/mpd/socket.\
                if both path and socket address are specified, then path has higher priority.
                If  this flag is set then music directory is automatically taken from mpd")
            )
        .arg(Arg::new("root-dir")
            .short('r')
            .long("root-dir")
            .takes_value(true)
            .validator(|pth|{
                if Path::new(&pth).is_dir(){
                Ok(())
            }else{
                Err(format!("invalid root-dir {}", pth))
            }
            })
            .help("mpd music directory")
            )
        .arg(Arg::new("socket-address")
            .short('a')
            .long("socket-address")
            .default_value("127.0.0.1:6600")
            .takes_value(true)
            .help("mpd socket address. <host>:<port> ex. -a 127.0.0.1:6600 \
                default value is 127.0.0.1:6600\
                ")
            )
        .subcommand(
            Command::new("listen")
            .short_flag('L')
            .long_flag("listen")
            .about("listens for mpd events")
        )
        .subcommand(
            Command::new("get-stats")
            .short_flag('G')
            .long_flag("get-stats")
            .about("get the stats of a specific song")
            .arg(
                Arg::new("current")
                .short('c')
                .long("current")
                .takes_value(false)
                .help("prints stats of a current song")
                )
            .arg(
                Arg::new("reverse")
                .short('r')
                .long("reverse")
                .takes_value(false)
                .help("reverse the order of list is printed")
                )
            .arg(
                Arg::new("previous")
                .short('p')
                .long("prev")
                .takes_value(false)
                .help("previous song")
                )
            .arg(
                Arg::new("next")
                .short('n')
                .long("next")
                .takes_value(false)
                .help("next song")
                )
            .arg(
                Arg::new("playlist")
                .short('P')
                .takes_value(true)
                .multiple_occurrences(true)
                .long("playlist")
                .help("prints the stats for the whole playlist")
                )
            .arg(
                Arg::new("queue")
                .short('Q')
                .long("queue")
                .help("prints the stats for current playing playlist/queue")
                )
            .arg(Arg::new("stats")
                .short('s')
                .long("stats")
                .help("prints the exact stats instead of a single rating number")
                )
            .arg(
                Arg::new("json")
                .short('j')
                .long("json")
                .requires("stats")
                .help("print stats in json format")
                )
            .arg(
                Arg::new("path")
                .multiple_values(true)
                .help("relative path from music directory configured in mpd")
                // TODO: configure whether to use positional arguments or optional args
                )
            )
        .subcommand(
            Command::new("set-stats")
            .short_flag('S')
            .long_flag("set-stats")
            .about("manually set stats for a perticular song, it should be in json")
            .arg(
                Arg::new("current")
                .short('c')
                .long("current")
                .takes_value(false)
                .help("prints stats of a current song")
                )
            .arg(
                Arg::new("path")
                .required_unless_present("current")
                .multiple_values(false)
                .help("relative path from music directory configured in mpd")
                // TODO: configure whether to use positional arguments or optional args
                )
            .arg(
                Arg::new("skip_cnt")
                .short('u')
                .long("skip-count")
                .takes_value(true)
                .conflicts_with("stats")
                .help("set the skip count for the song")
                )
            .arg(
                Arg::new("play_cnt")
                .short('p')
                .long("play-count")
                .takes_value(true)
                .conflicts_with("stats")
                .help("set the play count for the song")
                )
            .arg(
                Arg::new("stats")
                .short('s')
                .long("stats")
                .takes_value(true)
                .required_unless_present_any(&["play_cnt","skip_cnt"])
                .help("stats in json format. example: {\"play_cnt\":11,\"skip_cnt\":0}")
                )
            )
        .subcommand(
            Command::new("export")
            .short_flag('E')
            .long_flag("export")
            .about("export stats to a file")
            .arg(
                Arg::new("out-file")
                .required(false)
                .short('o')
                .long("out-file")
                .takes_value(true)
                .value_parser(clap::value_parser!(String))
                .help("output file[default it write to stdout]")
                )
            .arg(
                Arg::new("hash")
                .short('H')
                .long("hash")
                .takes_value(false)
                .help("exports with songs hash. this way songs name is not required to be matching")
                )
            )
        .subcommand(
            Command::new("import")
            .short_flag('I')
            .long_flag("import")
            .about("import stats from a file")
            .arg(
                Arg::new("hash")
                .short('H')
                .long("hash")
                .takes_value(false)
                .help("imports hashes as input, songs need to have the same name as exported ones. but it supports only for tags not for stickers")
                )
            .arg(
                Arg::new("input-file")
                .short('i')
                .long("input-file")
                .required(false)
                .takes_value(true)
                .value_parser(clap::value_parser!(String))
                .help("file containing stats")
                )
            )
        .subcommand(
            Command::new("clear")
            .long_flag("clear-stats")
            .about("resets all stats to 0")
            .arg(
                Arg::new("confirm")
                .short('y')
                .takes_value(false)
                .long("yes")
                .help("yes to confirm. dont ask for prompt")
                )
            )
        .get_matches();

    // set the verbosity
    match arguments.occurrences_of("verbose") {
        0 => builder
            .filter_module("mp_rater", log::LevelFilter::Error)
            .init(),
        1 => builder
            .filter_module("mp_rater", log::LevelFilter::Warn)
            .init(),
        2 => builder
            .filter_module("mp_rater", log::LevelFilter::Info)
            .init(),
        3 => builder
            .filter_module("mp_rater", log::LevelFilter::Debug)
            .init(),
        4 => builder
            .filter_module("mp_rater", log::LevelFilter::Trace)
            .init(),
        _ => {
            builder.filter_level(log::LevelFilter::Trace).init();
            trace!("wait one of the rust expert is coming to debug");
        }
    }
    debug!("log_level set to {:?}", log::max_level());

    let get_sock = || {
        let address = arguments.value_of("socket-address").unwrap();
        debug!("connecting to TcpStream {}", address);
        ConnType::Socket(std::net::TcpStream::connect(address).unwrap())
    };

    // if the socket address is manually given then use socket address only
    let con_t = if let Some(stream_path) = arguments.value_of("socket-path") {
        debug!("connecting to unix stream {}", stream_path);
        std::os::unix::net::UnixStream::connect(stream_path)
            .map_or_else(|_| get_sock(), ConnType::Stream)
    } else {
        get_sock()
    };
    let mut client = mpd::Client::new(con_t).unwrap();
    let use_tags = arguments.is_present("use-tags");
    if use_tags {
        if arguments.is_present("socket-path") {
            ROOT_DIR.set(client.music_directory().unwrap()).unwrap();
        } else if arguments.is_present("root-dir") {
            ROOT_DIR
                .set(arguments.value_of("root-dir").unwrap().to_string())
                .unwrap();
        } else {
            error!("root dir is not found, either use socket-path or mention root_dir manually");
            exit(1);
        }
    }
    match arguments.subcommand() {
        Some(("listen", subm)) => listener::listen(&mut client, subm, use_tags),
        Some(("get-stats", subm)) => stats::get_stats(&mut client, subm, use_tags),
        Some(("set-stats", subm)) => stats::set_stats(&mut client, subm, use_tags),
        Some(("import", subm)) => stats::import_stats(&mut client, subm, use_tags, arguments.contains_id("confirm")),
        Some(("export", subm)) => stats::export_stats(&mut client, subm, use_tags),
        Some(("clear", subm)) => stats::clear_stats(&mut client, subm, use_tags, arguments.contains_id("confirm")),
        _ => {}
    }
}
