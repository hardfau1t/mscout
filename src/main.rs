mod listener;
mod stats;
use clap::{App, Arg};
use log::{debug, trace};
use std::path::Path;

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
        .arg(Arg::new("socket_path")
             .short('p')
             .long("socket-path")
             .conflicts_with("socket_addr")
             .takes_value(true)
             .required_unless_present("socket_addr")
             .validator(|pth|{
                 if Path::new(&pth).exists(){
                     Ok(())
                 }else{
                     Err(format!("could get the socket {}", pth))
                 }
             })
             .help("path to mpd socket. If  this flag is set then music directory is automatically taken from mpd")
             )
        .arg(Arg::new("socket_addr")
             .short('a')
             .long("socket-address")
             .required_unless_present("socket_path")
             .conflicts_with("socket_path")
             .help("mpd socket address. <host>:<port> ex. -a 127.0.0.1:6600")
             )
        .subcommand(
            App::new("listen")
            .short_flag('l')
            .long_flag("listen")
            .about("listens for mpd events")
            .arg(
                Arg::new("mpd_database")
                .short('m')
                .long("use-mpd")
                .help("use mpd database to store statistics as stickers")
             )
            .arg(
                Arg::new("tag_database")
                .short('t')
                .long("use-tags")
                .help("use eyed3 tags to store ratings. These will be stored in comments")
                )
            )
        .subcommand(
            App::new("get-stats")
            .short_flag('g')
            .long_flag("get-stats")
            .about("get the stats of a specific song")
            .arg(
                Arg::new("human-readable")
                .short('r')
                .long("human-readable")
                .help("print stats in human-readable format")
                )
            .arg(
                Arg::new("json")
                .short('j')
                .long("json")
                .help("print stats in json format")
                )
            .arg(
                Arg::new("path")
                .help("relative path from music directory configured in mpd")
                // TODO: configure whether to use positional arguments or optional args
                )
            .arg(
                Arg::new("append-to-playlist")
                .short('a')
                .long("add-playlist")
                .help("appends to current playlist, if playlist is given then if playlist exists then appends to that playlist else creates a new playlist with that name")
                // TODO: optional name of playlist?
                )
            )
        .subcommand(
            App::new("set-stats")
            .short_flag('s')
            .long_flag("set-stats")
            .about("manually set stats for a perticular song, it should be in json")
            .arg(
                Arg::new("path")
                .help("relative path from music directory configured in mpd")
                // TODO: configure whether to use positional arguments or optional args
                )
            .arg(
                Arg::new("stats")
                .short('s')
                .long("stats")
                .help("stats in json format")
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

  let conn = if arguments.is_present("socket_path") {
    let stream = arguments.value_of("socket_path").unwrap();
    debug!("connecting to unix stream {}", stream);
    listener::ConnType::Stream(std::os::unix::net::UnixStream::connect(stream).unwrap())
  } else if arguments.is_present("socket-address") {
    let address = arguments.value_of("socket-address").unwrap();
    debug!("connecting to TcpStream {}", address);
    listener::ConnType::Socket(std::net::TcpStream::connect(address).unwrap())
  } else {
    unreachable!()
  };
  let mut comm = listener::Listener::new(conn).unwrap();
  match arguments.subcommand() {
    Some(("listen", subm)) => listener::listen(&mut comm, subm),
    Some(("get-stats", subm)) => stats::get_stats(&comm, subm),
    Some(("set-stats", subm)) => stats::set_stats(&comm, subm),
    _ => {}
  }
}
