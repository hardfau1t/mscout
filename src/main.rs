use clap::{App, Arg};
use env_logger;
use id3::Tag;
use socket2::{Socket, Domain, Type,SockAddr};
use log::{debug, error, info, trace, warn};
use mpd::{idle::Subsystem, status::State, Idle, Song, Status};
use std::process::exit;
use std::time::{Duration, Instant};

#[derive(Debug)]
enum Action {
    Skipped,
    Played,
}

#[derive(Debug)]
enum Operation {
    Add(u16),
    Subtract(u16),
    Reset,
}

struct Listener {
    client: mpd::Client<Socket>,
    last_state: Status,
    last_song: Option<Song>,
    timer: Instant,
    start_time: Duration,
}

impl Listener {
    fn new<A: std::net::ToSocketAddrs>(address: A) -> Result<Self, mpd::error::Error> {
        let addr = SockAddr::unix("/home/gireesh/.local/run/mpd/socket").unwrap();
        let sock = socket2::Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        sock.connect(&addr).unwrap();
        let mut client = mpd::Client::new(sock)?;
        let status = client.status()?;
        let timer = Instant::now();
        let last_song = client.currentsong().unwrap();
        Ok(Listener {
            client,
            last_state: status,
            timer,
            start_time: timer.elapsed(),
            last_song,
        })
    }
    fn add_sticker(&self, sticker_type: Action, op: Operation) {
        info!(
            "appending to sticker {:?} to {:?} ",
            sticker_type, self.last_state.song
        );
    }
    fn add_tag(&self, tag_type: Action, op: Operation) {
        info!(
            "appending to tag {:?} to {:?} ",
            tag_type, self.last_state.song
        );

        println!("path is {:#?}", self.last_song.as_ref().unwrap());
    }
    fn player_event(&mut self) {
        trace!("handling player_event()");
        let status = self.client.status().unwrap();
        // if state is paused or stopped then no need to rate. if last state is paused then its
        // just start so no need to rate either
        if status.state == State::Stop
            || status.state == State::Pause
            || self.last_state.state == State::Stop
        {
            trace!("ignoring player event");
            return;
        }
        // if its paused and resume then no need to rate. if paused and now its next song then its
        // been skipped
        if self.last_state.state == State::Pause {
            if self.last_state.song == status.song {
                trace!("player event: probably seeked");
                return;
            } else if self.last_state.nextsong == status.song {
                self.add_sticker(Action::Skipped, Operation::Add(1));
            }
        } else {
            // last state is playing and current is also playing and last_state next song is
            // current song then either its skipped or played completely
            if self.last_state.nextsong == status.song {
                if let Some(time_elapsed) = self.last_state.elapsed {
                    if let Ok(elapsed) = time_elapsed.to_std() {
                        // +1 second is kept to compunsate
                        let elapsed_time =
                            elapsed + self.timer.elapsed() - self.start_time + Duration::new(1, 0);
                        debug!(
                            "elapsed {:?}, timer_elapsed {:?}, start_time {:?}, duration {:?}, sum_played {:?}",
                            elapsed,
                            self.timer.elapsed(),
                            self.start_time,
                            self.last_state.duration,
                            elapsed_time
                        );
                        if elapsed_time >= self.last_state.duration.unwrap().to_std().unwrap() {
                            self.add_tag(Action::Played, Operation::Add(1));
                        } else {
                            self.add_tag(Action::Skipped, Operation::Add(1));
                        }
                    }
                }
                debug!("last_state elapsed is {:?}",self.last_state.elapsed);
                }
            else {
                debug!("probably changed the sequence");
            }
        }

    }
    fn listen(&mut self) -> ! {
        loop {
            self.last_state = self.client.status().unwrap();
            self.last_song = self.client.currentsong().unwrap();
            self.start_time = self.timer.elapsed();
            if let Ok(sub_systems) = self.client.wait(&[]) {
                for system in sub_systems {
                    match system {
                        Subsystem::Player => {
                            self.player_event()
                        },
                        _ => trace!("ignoring event {}", system),
                    }
                }
            }
        }
    }
}

fn args_handle() {
    let mut builder = env_logger::builder();
    let arguments = App::new("mp rater")
        .version("0.1.0")
        .author("hardfau18 <the.qu1rky.b1t@gmail.com>")
        .about("rates song with skip/rate count for mpd")
        .arg(
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .long("verbose")
                .help("sets the verbose level, use multiple times for more verbosity"),
        )
        .get_matches();
    match arguments.occurrences_of("v") {
        0 => builder.filter_level(log::LevelFilter::Warn).init(),
        1 => builder.filter_level(log::LevelFilter::Info).init(),
        2 => builder.filter_level(log::LevelFilter::Debug).init(),
        3 => builder.filter_level(log::LevelFilter::Trace).init(),
        _ => {
            builder.filter_level(log::LevelFilter::Trace).init();
            trace!("wait one of the rust expert is coming to debug");
        }
    }
    debug!("log_level set to {:?}", log::max_level());
}
fn main() {
    args_handle();
    let mut client = Listener::new(("127.0.0.1", 6600)).unwrap_or_else(|error| {
        error!("failed to get listener {:?}", error);
        exit(1);
    });
    println!("{:?}", client.client.music_directory());
    client.listen();
}
