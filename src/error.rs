use std::fmt;

/// The [`Limits`](crate::Limits) field that a call would pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limit {
    /// [`Limits::max_alloc`](crate::Limits::max_alloc).
    Alloc,
    /// [`Limits::max_pixels`](crate::Limits::max_pixels).
    Pixels,
    /// [`Limits::max_xmp_bytes`](crate::Limits::max_xmp_bytes).
    XmpBytes,
    /// [`Limits::max_segments`](crate::Limits::max_segments).
    Segments,
    /// [`Limits::max_images`](crate::Limits::max_images).
    Images,
}

/// Every error of this crate. Limit errors are one variant with a [`Limit`] so a caller can
/// match on the limit. Errors of the nested crates keep their own type, except the limit,
/// out-of-memory and cancel errors, which are mapped to ours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input is not a JPEG, HEIF or AVIF file.
    UnknownFormat,
    /// The input ends inside a structure that the operation needs.
    Truncated,
    /// A structure is invalid. The text names it: a short stable token, not a sentence.
    Malformed(&'static str),
    /// A feature this crate does not support (for example `"hevc decode"`).
    Unsupported(&'static str),
    /// A [`Limits`](crate::Limits) field would be passed.
    LimitExceeded(Limit),
    /// The allocator refused a size that is within the limits.
    OutOfMemory,
    /// The cancel flag was set.
    Cancelled,
    /// An error of the JPEG decoder.
    Jpeg(jpeg::Error),
    /// An error of the HEIF parser.
    Heif(isobmff::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => f.write_str("unknown format"),
            Self::Truncated => f.write_str("truncated input"),
            Self::Malformed(what) => write!(f, "malformed {what}"),
            Self::Unsupported(what) => write!(f, "unsupported {what}"),
            Self::LimitExceeded(limit) => write!(f, "limit exceeded: {limit:?}"),
            Self::OutOfMemory => f.write_str("out of memory"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Jpeg(e) => write!(f, "jpeg: {e}"),
            Self::Heif(e) => write!(f, "heif: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Jpeg(e) => Some(e),
            Self::Heif(e) => Some(e),
            _ => None,
        }
    }
}

impl From<jpeg::Error> for Error {
    fn from(e: jpeg::Error) -> Self {
        match e {
            jpeg::Error::AllocLimit { .. } => Self::LimitExceeded(Limit::Alloc),
            jpeg::Error::PixelLimit { .. } => Self::LimitExceeded(Limit::Pixels),
            jpeg::Error::OutOfMemory => Self::OutOfMemory,
            jpeg::Error::Cancelled => Self::Cancelled,
            e => Self::Jpeg(e),
        }
    }
}

impl From<isobmff::Error> for Error {
    fn from(e: isobmff::Error) -> Self {
        match e {
            isobmff::Error::LimitExceeded(isobmff::Limit::Alloc) => {
                Self::LimitExceeded(Limit::Alloc)
            }
            isobmff::Error::LimitExceeded(isobmff::Limit::Pixels) => {
                Self::LimitExceeded(Limit::Pixels)
            }
            isobmff::Error::OutOfMemory => Self::OutOfMemory,
            isobmff::Error::Cancelled => Self::Cancelled,
            e => Self::Heif(e),
        }
    }
}

impl Error {
    /// A short stable token per variant (`unknown-format`, `truncated`, `malformed`,
    /// `unsupported`, `limit-<field>`, `out-of-memory`, `cancelled`, `jpeg`, `heif`). The parity
    /// oracle writes it.
    pub fn class(&self) -> &'static str {
        match self {
            Self::UnknownFormat => "unknown-format",
            Self::Truncated => "truncated",
            Self::Malformed(_) => "malformed",
            Self::Unsupported(_) => "unsupported",
            Self::LimitExceeded(limit) => match limit {
                Limit::Alloc => "limit-alloc",
                Limit::Pixels => "limit-pixels",
                Limit::XmpBytes => "limit-xmp-bytes",
                Limit::Segments => "limit-segments",
                Limit::Images => "limit-images",
            },
            Self::OutOfMemory => "out-of-memory",
            Self::Cancelled => "cancelled",
            Self::Jpeg(_) => "jpeg",
            Self::Heif(_) => "heif",
        }
    }
}
