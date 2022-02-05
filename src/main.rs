#![warn(missing_docs)]
#![warn(clippy::missing_docs_in_private_items)]

//! This crate provides a way to set or get ratings for songs based on listening statistics.
//! This is written for mpd as plugin. To work you have to have mpd running.
mod listener;
mod stats;
use clap::{App, Arg};
use log::{debug, error, trace};
use once_cell::sync::OnceCell;
use std::path::Path;
use std::process::exit;

/// contains root dir string optionally either if the user passes through cmdline or if the unix
/// socket file is given
static ROOT_DIR: OnceCell<String> = OnceCell::new();

fn main() {
  let mut builder = env_logger::builder();
  let arguments = App::new("mp rater")
        .version("0.1.0")
        .author("hardfau18 <the.qu1rky.b1t@gmail.com>")
        .about("rates song with skip/rate count for mpd")
        .arg(
            Arg::new("verbose")
                .short('v')
                .multiple_occurrences(true)
                .long("verbose")
                .help("sets the verbose level, use multiple times for more verbosity")
        )
            .arg(
                Arg::new("use-tags")
                .short('t')
                .long("use-tags")
                .help("use eyed3 tags to store ratings. If not specified by default mpd stickers are used. tags are persistante across file moves, where as incase of mpd sticker these will be erased if you move the files.")
                )
        .arg(Arg::new("socket-path")
             .short('p')
             .long("socket-path")
             .conflicts_with("socket-address")
             .takes_value(true)
             .required_unless_present("socket-address")
             .validator(|pth|{
                 if Path::new(&pth).exists(){
                     Ok(())
                 }else{
                     Err(format!("could get the socket {}", pth))
                 }
             })
             .help("path to mpd socket. If  this flag is set then music directory is automatically taken from mpd")
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
             .help("root directory of mpd server.")
             )
        .arg(Arg::new("socket-address")
             .short('a')
             .long("socket-address")
             .required_unless_present("socket-path")
             .conflicts_with("socket-path")
             .takes_value(true)
             .help("mpd socket address. <host>:<port> ex. -a 127.0.0.1:6600")
             )
        .subcommand(
            App::new("listen")
            .short_flag('L')
            .long_flag("listen")
            .about("listens for mpd events")
        )
        .subcommand(
            App::new("get-stats")
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
            App::new("set-stats")
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
                Arg::new("stats")
                .short('s')
                .long("stats")
                .takes_value(true)
                .required(true)
                .help("stats in json format. example: {\"play_cnt\":11,\"skip_cnt\":0}")
                // TODO: add an example
                )
            )
        .get_matches();

  // set the verbosity
  match arguments.occurrences_of("verbose") {
    0 => builder.filter_level(log::LevelFilter::Error).init(),
    1 => builder.filter_level(log::LevelFilter::Warn).init(),
    2 => builder.filter_level(log::LevelFilter::Info).init(),
    3 => builder.filter_level(log::LevelFilter::Debug).init(),
    4 => builder.filter_level(log::LevelFilter::Trace).init(),
    _ => {
      builder.filter_level(log::LevelFilter::Trace).init();
      trace!("wait one of the rust expert is coming to debug");
    }
  }
  debug!("log_level set to {:?}", log::max_level());

  let con_t = if arguments.is_present("socket-path") {
    let stream = arguments.value_of("socket-path").unwrap();
    debug!("connecting to unix stream {}", stream);
    listener::ConnType::Stream(std::os::unix::net::UnixStream::connect(stream).unwrap())
  } else if arguments.is_present("socket-address") {
    let address = arguments.value_of("socket-address").unwrap();
    debug!("connecting to TcpStream {}", address);
    listener::ConnType::Socket(std::net::TcpStream::connect(address).unwrap())
  } else {
    unreachable!()
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
    _ => {}
  }
}
