use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DecoderError {
    #[error("inspektor only support postgres version 3")]
    UnsupporedVersion,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
    #[error("io error")]
    IoErr(#[from] io::Error),
}
