/// Caps on one call. `read`, `decode`, `GainMapImage::coded_bytes` and `IsoMetadata::parse` take
/// `&Limits` (the first three also a cancel flag); there is no unlimited entry point. The
/// defaults are finite and large enough for real phone photos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The peak working memory of one call, in bytes (default 1 GiB), all threads together and
    /// including the nested calls into `jpeg`, `exif`, `isobmff`, `hevc` and `av1`, which get what is
    /// left. Every buffer whose size comes from the input is charged before it is allocated. More is
    /// [`Error::LimitExceeded`](crate::Error::LimitExceeded)`(`[`Limit::Alloc`](crate::Limit::Alloc)`)`.
    pub max_alloc: usize,
    /// The most OS threads one call uses, including the calling thread (default
    /// `std::thread::available_parallelism()`). This crate spawns none; the decode of
    /// [`decode`](crate::decode) passes the value on to `jpeg` and `av1` (`hevc` decodes serially).
    /// 1 runs on the calling thread.
    pub max_threads: usize,
    /// The most pixels (width times height) of one image (default 2^30).
    /// [`Limit::Pixels`](crate::Limit::Pixels).
    pub max_pixels: u64,
    /// The largest XMP packet, in bytes (default 16 MiB).
    /// [`Limit::XmpBytes`](crate::Limit::XmpBytes).
    pub max_xmp_bytes: usize,
    /// The most markers walked in one JPEG image (default 100 000).
    /// [`Limit::Segments`](crate::Limit::Segments).
    pub max_segments: usize,
    /// The most MPF entries and the most candidate gain-map images (default 64).
    /// [`Limit::Images`](crate::Limit::Images).
    pub max_images: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_alloc: 1 << 30,
            max_threads: std::thread::available_parallelism().map_or(1, usize::from),
            max_pixels: 1 << 30,
            max_xmp_bytes: 16 << 20,
            max_segments: 100_000,
            max_images: 64,
        }
    }
}
