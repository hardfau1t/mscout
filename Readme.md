# mscout
Its a plugin for mpd, which handles rating for each song based on play count and skip counts.

NOTE: This doesn't work if consume is enabled

----
## Requirements
1. mpd
2. rust `<optional only if building from source>`

----

## Installation
building from source
* `Git clone https://github.com/hardfau18/mscout.git`
* `cd mscout`
* `cargo build --release`
* `cp target/release/mscout ~/.local/bin`
* if `~/.local/bin/` path is not set then `export PATH=~/.local/bin:$PATH`. To make it permanent add it in `~/.bashrc` file.

## Running
To run `mscout`, `mpd` should be running in background and should be listening on the local network socket or unix socket file.
To configure mpd to listening on network socket add these two lines to mpd.conf file.
```
bind_to_address   "any"
port              "6600"
```
To listen on unix socket file put the below line. make sure that `~/.local/run/mpd` folder exists if not create it by `mkdir ~/.local/run/mpd`.
```
bind_to_address		"~/.local/run/mpd/socket"
```

There are 2 ways to store stats of songs.
1. Using mpd sticker database to hold ratings
2. Using songs id3 tags to store ratings
By default mpd database is used to store ratings. But this is not persistent. If you move any files to a separate directories then all of the ratings of those files will reset.
id3 tags store the rating even if you move the songs. ratings will be saved in comment section of id3 tag of respective song. If you want to use id3 tags use `--socket-path <path to socket file>` option or `--root-dir <path to mpd music directory>` and with that `--use-tags` option.

If you don't want give `--use-tags` each time you can `export MSCOUT_USE_TAGS=1` variable.


#### examples: 
For listening on network socket with mpd  sticker database

`mscout -a 127.0.0.1:6600 -L`

For listening on network socket with id3 tags

`mscout -a 127.0.0.1:6600 --use-tags -r <mpd music directory > -L`

For listening on socket file with id3 tags

`mscout -p ~/.local/run/mpd/socket --use-tags -L`

### retrieving ratings.
To get rating for a particular song use get-stats option. For example to get stats for current song

`mscout -a 127.0.0.1:6600 -G --current`

Or to get stats for any other song, give a path relative to mpd music directory

`mscout -a 127.0.0.1:6600 -G <relative path to song>`

To get stats from whole playlist.

`mscout -G --playlist <playlist> ...`

To get stats from currrent queue.

`mscout -G -Q `

use -s flags to get exact play and skip count
