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
    /// An error of the HEVC decoder (feature `heif`).
    #[cfg(feature = "heif")]
    Hevc(hevc::Error),
    /// An error of the AV1 decoder (feature `avif`).
    #[cfg(feature = "avif")]
    Av1(av1::Error),
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
            #[cfg(feature = "heif")]
            Self::Hevc(e) => write!(f, "{e}"),
            #[cfg(feature = "avif")]
            Self::Av1(e) => write!(f, "av1: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Jpeg(e) => Some(e),
            Self::Heif(e) => Some(e),
            #[cfg(feature = "heif")]
            Self::Hevc(e) => Some(e),
            #[cfg(feature = "avif")]
            Self::Av1(e) => Some(e),
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
    /// `unsupported`, `limit-<field>`, `out-of-memory`, `cancelled`, `jpeg`, `heif`, `hevc`,
    /// `av1`). The parity oracle writes it.
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
            #[cfg(feature = "heif")]
            Self::Hevc(_) => "hevc",
            #[cfg(feature = "avif")]
            Self::Av1(_) => "av1",
        }
    }
}

#[cfg(feature = "heif")]
impl From<hevc::Error> for Error {
    fn from(e: hevc::Error) -> Self {
        match e {
            hevc::Error::AllocLimit => Self::LimitExceeded(Limit::Alloc),
            hevc::Error::PixelLimit | hevc::Error::WidthLimit | hevc::Error::HeightLimit => {
                Self::LimitExceeded(Limit::Pixels)
            }
            hevc::Error::OutOfMemory => Self::OutOfMemory,
            hevc::Error::Cancelled => Self::Cancelled,
            e => Self::Hevc(e),
        }
    }
}

#[cfg(feature = "avif")]
impl From<av1::Error> for Error {
    fn from(e: av1::Error) -> Self {
        match e {
            // The metadata cap is carved out of `max_alloc` by `decode`.
            av1::Error::AllocLimit | av1::Error::MetadataLimit => Self::LimitExceeded(Limit::Alloc),
            av1::Error::PixelLimit | av1::Error::WidthLimit | av1::Error::HeightLimit => {
                Self::LimitExceeded(Limit::Pixels)
            }
            av1::Error::OutOfMemory => Self::OutOfMemory,
            av1::Error::Cancelled => Self::Cancelled,
            // `av1` reports every `isobmff` error as `Container(class)`, limits included.
            av1::Error::Container("limit-alloc") => Self::LimitExceeded(Limit::Alloc),
            av1::Error::Container("limit-pixels") => Self::LimitExceeded(Limit::Pixels),
            av1::Error::Container("out-of-memory") => Self::OutOfMemory,
            av1::Error::Container("cancelled") => Self::Cancelled,
            e => Self::Av1(e),
        }
    }
}

#[cfg(all(test, any(feature = "heif", feature = "avif")))]
mod tests {
    use super::*;

    #[cfg(feature = "heif")]
    #[test]
    fn hevc_errors_map_to_ours() {
        use hevc::Error as E;
        assert_eq!(
            Error::from(E::AllocLimit),
            Error::LimitExceeded(Limit::Alloc)
        );
        for e in [E::PixelLimit, E::WidthLimit, E::HeightLimit] {
            assert_eq!(Error::from(e), Error::LimitExceeded(Limit::Pixels));
        }
        assert_eq!(Error::from(E::OutOfMemory), Error::OutOfMemory);
        assert_eq!(Error::from(E::Cancelled), Error::Cancelled);
        let e = Error::from(E::InvalidData("x"));
        assert_eq!((e.class(), e), ("hevc", Error::Hevc(E::InvalidData("x"))));
        assert_eq!(Error::from(E::InterSlice).class(), "hevc");
    }

    #[cfg(feature = "avif")]
    #[test]
    fn av1_errors_map_to_ours() {
        use av1::Error as E;
        for e in [E::AllocLimit, E::MetadataLimit] {
            assert_eq!(Error::from(e), Error::LimitExceeded(Limit::Alloc));
        }
        for e in [E::PixelLimit, E::WidthLimit, E::HeightLimit] {
            assert_eq!(Error::from(e), Error::LimitExceeded(Limit::Pixels));
        }
        assert_eq!(Error::from(E::OutOfMemory), Error::OutOfMemory);
        assert_eq!(Error::from(E::Cancelled), Error::Cancelled);
        assert_eq!(Error::from(E::Truncated), Error::Av1(E::Truncated));
        assert_eq!(
            Error::from(E::Container("limit-alloc")),
            Error::LimitExceeded(Limit::Alloc)
        );
        assert_eq!(
            Error::from(E::Container("limit-pixels")),
            Error::LimitExceeded(Limit::Pixels)
        );
        assert_eq!(Error::from(E::Container("cancelled")), Error::Cancelled);
        assert_eq!(Error::from(E::Container("truncated")).class(), "av1");
        assert_eq!(Error::from(E::InterFrame).class(), "av1");
    }
}
