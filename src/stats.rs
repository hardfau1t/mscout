//! This module has functions related to statitics, manually setting them and displaying them.
use crate::{
    error::{CustomEror, Error},
    ConnType, MP_DESC, ROOT_DIR,
};
use id3::{frame::Comment, Tag};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::{path, process::exit};

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
            - self.skip_cnt as f32
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
            |err| {
                debug!("error {err} while getting stats");
                match err {
                    mpd::error::Error::Parse(_) => Ok(Statistics {
                        play_cnt: 0,
                        skip_cnt: 0,
                    }),
                    mpd::error::Error::Server(_) => Err(Error::FileNotExists),
                    _ => Err(Error::ConnectionFailed),
                }
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
    let song_pbuff = if rel_path.is_file() {
        path::PathBuf::from(rel_path)
    } else {
        path::PathBuf::from(ROOT_DIR.get().expect("statistics to tag requires full path, try to use --socket-file or set root-dir manually")).join(rel_path)
    };
    let mut cmt = None;
    debug!("songs full path is {:#?}", song_pbuff);
    let tag = Tag::read_from_path(&song_pbuff).or_else(|err: id3::Error| match err.kind {
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
    let song_pbuff = if spath.is_file() {
        path::PathBuf::from(spath)
    } else {
        path::PathBuf::from(ROOT_DIR.get().expect("statistics to tag requires full path, try to use --socket-file or set root-dir manually")).join(spath)
    };
    debug!("setting tag to {:#?}", song_pbuff);
    let mut tag = Tag::read_from_path(&song_pbuff).or_else(|err: id3::Error| match err.kind {
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
    tag.write_to_path(&song_pbuff, id3::Version::Id3v24)
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
    }
    let queue = client
        .queue()
        .try_unwrap("Couldn't get the queue information from mpd");
    if args.is_present("previous") {
        if let Some(cur) = client
            .currentsong()
            .try_unwrap("Failed to get current song")
        {
            let mut q_iter = queue.iter();
            if let Some(mut prev) = q_iter.next() {
                for s in q_iter {
                    if s.place.unwrap().id == cur.place.unwrap().id {
                        songs.push(path::PathBuf::from(&prev.file));
                        break;
                    }
                    prev = s;
                }
            }
        } else {
            error!("Couldn't get the previous song");
            dbg!("Current song is empty");
        }
    }
    if args.is_present("next") {
        if let Some(cur) = client
            .currentsong()
            .try_unwrap("Failed to get current song")
        {
            let mut q_iter = queue.iter();
            for s in q_iter.by_ref() {
                if s.place.unwrap().id == cur.place.unwrap().id {
                    if let Some(next) = q_iter.next() {
                        songs.push(path::PathBuf::from(&next.file));
                    } else {
                        if q_iter.count() == 0 {
                            dbg!("Couldn't get the current song");
                        }
                        error!("Couldn't get the next song");
                    }
                    break;
                }
            }
        } else {
            error!("Couldn't get the previous song");
            dbg!("Current song is empty");
        }
    }
    // Collect sogngs
    if let Some(playlists) = args.values_of("playlist") {
        for playlist in playlists {
            debug!("appending playlist {playlist} to songs list");
            match client.playlist(playlist) {
                Ok(pl_content) => {
                    for s_pth in pl_content {
                        debug!("appending song {} to songs", s_pth.file);
                        songs.push(path::PathBuf::from(s_pth.file));
                    }
                }
                Err(err) => error!("failed to add playlist due to {err}"),
            }
        }
    }
    if args.is_present("queue") {
        if let Ok(q) = client.queue() {
            for s_path in q {
                debug!("appending path {} to songs list", s_path.file);
                songs.push(path::PathBuf::from(s_path.file));
            }
        } else {
            error!("failed to get current queue");
        }
    };
    if let Some(s_paths) = args.values_of("path") {
        for user_path in s_paths {
            debug!("appending path {user_path} to songs list");
            songs.push(path::PathBuf::from(user_path));
        }
    };
    // Collect ratings
    let mut with_ratings: Vec<(_, _)> = Vec::new();
    for song in songs {
        if let Ok(rating) = if use_tags {
            stats_from_tag(&song)
        } else {
            stats_from_sticker(client, &song)
        } {
            with_ratings.push((
                song.to_str()
                    .expect("Failed to get the song name into string")
                    .to_owned(),
                rating,
            ));
        } else {
            error!("Couldn't get the stats for {song:?}");
        }
    }

    // Sort the songs by ratings
    with_ratings.sort_by(|s1, s2| {
        if args.is_present("reverse") {
            s2.1.get_ratings().partial_cmp(&s1.1.get_ratings()).unwrap()
        } else {
            s1.1.get_ratings().partial_cmp(&s2.1.get_ratings()).unwrap()
        }
    });
    // -------------- print all teh stats----------------------------
    for (song, rating) in with_ratings {
        if args.is_present("stats") {
            if args.is_present("json") {
                println!("{}", serde_json::to_string(&(&song, &rating)).unwrap());
            } else {
                println!(
                    "play count: {}\tskip count: {} - {}",
                    rating.play_cnt, rating.skip_cnt, song
                );
            }
        } else {
            println!("{} - {}", rating.get_ratings(), song);
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
        path::PathBuf::from(subc.value_of("path").unwrap()) // path is required variable so it can be unwrapped
    };
    // if json stats are given then get the stats from json. if not then pick the stats from file and update with given ones
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

/// imports stats from a given file
pub fn import_stats(client: &mut mpd::Client<ConnType>, subc: &clap::ArgMatches, use_tags: bool) {
}

/// exports all stats to a file
pub fn export_stats(client: &mut mpd::Client<ConnType>, subc: &clap::ArgMatches, use_tags: bool) {
    if subc.is_present("hash"){
        todo!()
    }
}
/// clears stats of all files
pub fn clear_stats(client: &mut mpd::Client<ConnType>, subc: &clap::ArgMatches, use_tags: bool) {
}
