//! module which implments error handling in this crate
use log::error;
use std::process::exit;

/// Error type
#[derive(Debug)]
pub enum Error {
    /// when the requested file doesn't exists.
    FileNotExists,
    /// Connection with mpd failed
    ConnectionFailed,
    /// unknown Error
    #[allow(dead_code)]         // for the future use
    Unknown,
}

/// Custom trait to implement standard expect method but does some logging and exits.
pub trait CustomEror<T> {
    /// if Ok then returns the value else does logging and returns.
    fn try_unwrap(self, err_msg: &str) -> T;
}

impl<T> CustomEror<T> for serde_json::Result<T> {
    fn try_unwrap(self, err_msg: &str) -> T {
        self.unwrap_or_else(|err| {
            match err.classify() {
                serde_json::error::Category::Syntax => {
                    error!(
                        "{}. invalid json syntax at {}:{}",
                        err_msg,
                        err.line(),
                        err.column()
                    );
                }
                serde_json::error::Category::Data => {
                    error!(
                        "{}, invalid input data format at {}:{}",
                        err_msg,
                        err.line(),
                        err.column()
                    );
                }
                _ => {
                    error!(
                        "{}, unknown json serialization or deserialization error",
                        err_msg
                    );
                }
            }
            exit(1);
        })
    }
}

impl<T> CustomEror<T> for mpd::error::Result<T> {
    fn try_unwrap(self, err_msg: &str) -> T {
        self.unwrap_or_else(|err| {
            match err {
                mpd::error::Error::Io(_) => error!("{}, may be connection failed", err_msg),
                mpd::error::Error::Server(s_err) => {
                    error!("{}, mpd server error {}", err_msg, s_err.detail)
                }
                _ => error!("{}, unknown mpd error!!", err_msg),
            }
            exit(1);
        })
    }
}
