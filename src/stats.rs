//! This module has functions related to statitics, manually setting them and displaying them.
use crate::{
    error::{CustomEror, Error},
    ConnType, MP_DESC, ROOT_DIR,
};
use id3::{frame::Comment, Tag};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::path;
use std::process::exit;

// #[derive(Debug)]
// enum Operation {
//   Add(u16),
//   Subtract(u16),
//   Reset,
// }

/// stores statistics in the form of played count and skipped count. using these perticular song
/// can be rated.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Statistics {
    /// number of times a song is played completely.
    play_cnt: u32,
    /// number of times a song is skipped.
    skip_cnt: u32,
}

impl Statistics {
    /// increments skip count
    pub fn skipped(&mut self) {
        self.skip_cnt += 1;
    }
    /// increments the play count
    pub fn played(&mut self) {
        self.play_cnt += 1;
    }
    /// returns ratings which is a number between 0-10 if there are ratings else None
    pub fn get_ratings(&self) -> f32 {
        (self.play_cnt as f32 / (1 + self.skip_cnt) as f32) * (self.play_cnt + self.skip_cnt) as f32
    }
}

/// gets the stats from mpd sticker database.
/// where spath is the path to the song relative to mpd's directory
pub fn stats_from_sticker(
    client: &mut mpd::Client<ConnType>,
    spath: &std::path::Path,
) -> Result<Statistics, Error> {
    trace!("getting stats from  mpd database for {:?}", spath);
    // get the stats from sticker, if not found then return 0,0
    client
        .sticker("song", spath.to_str().unwrap(), MP_DESC)
        .map_or_else(
            |err| match err {
                mpd::error::Error::Parse(_) => Ok(Statistics {
                    play_cnt: 0,
                    skip_cnt: 0,
                }),
                _ => Err(Error::ConnectionFailed),
            },
            |sticker| {
                Ok(serde_json::from_str(&sticker).unwrap_or_else(|err| {
                    warn!("couldn't parse sticker: {:?}", err);
                    client
                        .delete_sticker("song", spath.to_str().unwrap(), MP_DESC) // if the sticker is invalid then remove it.
                        .unwrap_or_else(|err| warn!("failed to delete sticker {:?}", err));
                    Statistics {
                        play_cnt: 0,
                        skip_cnt: 0,
                    }
                }))
            },
        )
}

/// set the stats to mpd sticker database.
/// where spath is the path to the song relative to mpd's directory
pub fn stats_to_sticker(
    client: &mut mpd::Client<ConnType>,
    spath: &std::path::Path,
    stats: &Statistics,
) -> Result<(), Error> {
    info!("setting stats {:?} to mpd database for {:?}", stats, spath);
    client
        .set_sticker(
            "song",
            spath.to_str().unwrap(),
            MP_DESC,
            &serde_json::to_string(stats).expect("Couldn't dump stats to json"),
        )
        .map_err(|err| {
            error!("Couldn't dump to mpd  database due to {:?}", err);
            Error::ConnectionFailed
        })?;
    Ok(())
}

/// extracts the statistics from eyed3 tags(from comments).
pub fn stats_from_tag(rel_path: &std::path::Path) -> Result<Statistics, Error> {
    let mut cmt = None;
    let mut spath = path::PathBuf::from(ROOT_DIR.get().expect(
        "statistics to tag requires full path, try to use --socket-file or set root-dir manually",
    ));
    spath.push(rel_path);
    debug!("songs full path is {:#?}", spath);
    let tag = Tag::read_from_path(&spath).or_else(|err: id3::Error| match err.kind {
        id3::ErrorKind::NoTag => {
            warn!("no tag found creating a new id3 tag");
            Ok(Tag::new())
        }
        _ => {
            error!(
                " error while opening tag {:?} for song {:?}",
                err.description, rel_path
            );
            Err(Error::FileNotExists)
        }
    })?;
    // return Err(Error::FileNotExists);
    for com in tag.comments() {
        debug!("available comments are {:?}", com);
        if com.description == MP_DESC {
            cmt = Some(com.clone());
            break;
        }
    }
    // if the file has ratings comment then modify it, else create fresh one with 0 0
    cmt.map_or(
        Ok(Statistics {
            play_cnt: 0,
            skip_cnt: 0,
        }),
        |comment| {
            let rating: Statistics = serde_json::from_str(&comment.text).unwrap_or_else(|err| {
                warn!(
                    "err {} invalid json text for rating comment {}",
                    err, comment.text
                );
                Statistics {
                    play_cnt: 0,
                    skip_cnt: 0,
                }
            });
            Ok(rating)
        },
    )
}

/// set the statistics to the eyed3 tags(from comments).
/// spath : absolute path to the song.
pub fn stats_to_tag(spath: &std::path::Path, stats: &Statistics) -> Result<(), Error> {
    let mut root = path::PathBuf::from(ROOT_DIR.get().expect(
        "statistics to tag requires full path, try to use --socket-file or set root-dir manually",
    ));
    root.push(spath);
    debug!("setting tag to {:#?}", root);
    let mut tag = Tag::read_from_path(&root).or_else(|err: id3::Error| match err.kind {
        id3::ErrorKind::NoTag => {
            warn!("no tag found creating a new id3 tag");
            Ok(Tag::new())
        }
        _ => {
            error!(" error while opening tag {:?}", err.description);
            Err(Error::FileNotExists)
        }
    })?;
    let comment: Comment = Comment {
        lang: "eng".to_string(),
        description: MP_DESC.to_string(),
        text: serde_json::to_string(stats).expect("couldn't convert ratings  to json"),
    };
    info!("attaching tag comment {:?}", comment);
    tag.add_comment(comment);
    tag.write_to_path(&root, id3::Version::Id3v24)
        .unwrap_or_else(|err| warn!("failed to write tag {}", err));
    Ok(())
}

/// extracts song statistics from id3 metadata or mpd's database based on use-tags flags
pub fn get_stats(client: &mut mpd::Client<ConnType>, args: &clap::ArgMatches, use_tags: bool) {
    let mut songs = Vec::new();
    if args.is_present("current") {
        songs.push(path::PathBuf::from(
            client
                .currentsong()
                .try_unwrap("failed to get current song")
                .unwrap_or_else(|| {
                    error!("failed to get current song from mpd");
                    exit(1); // exit if current song is not available
                })
                .file,
        ));
    } else {
        for user_path in args.values_of("path").unwrap() {
            songs.push(path::PathBuf::from(user_path));
        }
    };
    for song in songs {
        let rates = if use_tags {
            stats_from_tag(&song).unwrap_or_else(|err| {
                if let Error::FileNotExists = err {
                    error!("{:?} does'n exists", song);
                }
                exit(1);
            })
        } else {
            stats_from_sticker(client, &song).unwrap_or_else(|err| {
                error!("Couldn't get the sticker due to {:?}", err);
                exit(1);
            })
        };
        if args.is_present("stats") {
            if args.is_present("json") {
                println!("{}", serde_json::to_string(&rates).unwrap());
            } else {
                println!(
                    "play count: {}\nskip count {}",
                    rates.play_cnt, rates.skip_cnt
                );
            }
        } else {
            println!("ratings: {}", rates.get_ratings());
        }
    }
}

/// sets the stats of a custom user stats
pub fn set_stats(client: &mut mpd::Client<ConnType>, subc: &clap::ArgMatches, use_tags: bool) {
    // get the song to set stats, if current is given then get it from mpd or else from path
    // argument
    let song_file = if subc.is_present("current") {
        path::PathBuf::from(
            client
                .currentsong()
                .try_unwrap("failed to get current song")
                .unwrap_or_else(|| {
                    error!("failed to get current song from mpd");
                    exit(1);
                })
                .file,
        )
    } else {
        path::PathBuf::from(subc.value_of("path").unwrap()) // path is required variable
    };
    let stat = if subc.is_present("stats") {
        serde_json::from_str::<Statistics>(
            subc.value_of("stats")
                .expect("value of stats is not present, please report"),
        )
        .try_unwrap("error while parsing parsing Stats")
    } else {
        let mut curr_stat = if use_tags {
            stats_from_tag(&song_file).unwrap_or_else(|err| {
                if let Error::FileNotExists = err {
                    error!("{:?} does'n exists", song_file);
                }
                exit(1);
            })
        } else {
            stats_from_sticker(client, &song_file).unwrap_or_else(|err| {
                error!("Couldn't Get the stats from sticker: {:?}", err);
                exit(1);
            })
        };
        if subc.is_present("play_cnt") {
            curr_stat.play_cnt = subc
                .value_of("play_cnt")
                .unwrap() // required variable
                .parse()
                .expect("expected integer value for play_cnt");
        }
        if subc.is_present("skip_cnt") {
            curr_stat.skip_cnt = subc
                .value_of("skip_cnt")
                .unwrap() // required variable
                .parse()
                .expect("expected integer value for skip_cnt");
        }
        curr_stat
    };

    match if use_tags {
        stats_to_tag(&song_file, &stat)
    } else {
        stats_to_sticker(client, &song_file, &stat)
    } {
        Ok(_) => info!("stats {stat:?} set to {song_file:?}"),
        Err(_) => error!("Failed to set stats"),
    }
}
