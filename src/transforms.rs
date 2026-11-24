//! The opt-in `clap`, `irot` and `imir` step of [`decode_with`](crate::decode_with), through the
//! `isobmff` transform helper (`isobmff_tr`, a second rev, docs/dependencies.md). The `isobmff`
//! values in [`Planes`] come from the older rev that `hevc` and `av1` share, so they are converted
//! field by field.

use crate::budget::Budget;
use crate::heif::{IDEN_DEPTH, checked_image};
use crate::{Chroma, Error, Limit, Limits, Plane, Planes, Samples};

/// The transform of the rev that `hevc` and `av1` use, as the helper's type.
pub(crate) fn to_tr(t: &isobmff::Transform) -> isobmff_tr::Transform {
    match *t {
        isobmff::Transform::Clap(c) => isobmff_tr::Transform::Clap(isobmff_tr::Clap {
            width_n: c.width_n,
            width_d: c.width_d,
            height_n: c.height_n,
            height_d: c.height_d,
            horiz_off_n: c.horiz_off_n,
            horiz_off_d: c.horiz_off_d,
            vert_off_n: c.vert_off_n,
            vert_off_d: c.vert_off_d,
        }),
        isobmff::Transform::Rotate(deg) => isobmff_tr::Transform::Rotate(deg),
        isobmff::Transform::Mirror(axis) => isobmff_tr::Transform::Mirror(match axis {
            isobmff::Axis::TopBottom => isobmff_tr::Axis::TopBottom,
            isobmff::Axis::LeftRight => isobmff_tr::Axis::LeftRight,
        }),
    }
}

impl From<isobmff_tr::Error> for Error {
    /// The helper's limit, out-of-memory and cancel errors map to ours; `Malformed` and
    /// `Unsupported` keep their token. It reports nothing else for planes; a variant it might add
    /// is `Malformed("transform")`.
    fn from(e: isobmff_tr::Error) -> Self {
        match e {
            isobmff_tr::Error::LimitExceeded(isobmff_tr::Limit::Alloc) => {
                Self::LimitExceeded(Limit::Alloc)
            }
            isobmff_tr::Error::LimitExceeded(isobmff_tr::Limit::Pixels) => {
                Self::LimitExceeded(Limit::Pixels)
            }
            isobmff_tr::Error::OutOfMemory => Self::OutOfMemory,
            isobmff_tr::Error::Cancelled => Self::Cancelled,
            isobmff_tr::Error::Truncated => Self::Truncated,
            isobmff_tr::Error::Malformed(what) => Self::Malformed(what),
            isobmff_tr::Error::Unsupported(what) => Self::Unsupported(what),
            _ => Self::Malformed("transform"),
        }
    }
}

const TR_SIZE: usize = size_of::<isobmff_tr::Transform>();

/// The transforms of the `iden` items between the item `id` and what the codec decodes,
/// innermost first, as helper values; empty when `id` is not an `iden`. The returned `Vec` is
/// charged (`len * size`), and [`apply`] gives it back.
///
/// Also the check for a grid: the codec assembles a grid from raw tiles, so a tile with
/// transforms of its own cannot be transformed before the paste here, and that is
/// `Unsupported("grid-tile-transforms")` (the caller has not decoded anything yet).
pub(crate) fn inner_chain(
    heif: &isobmff::Heif,
    id: u32,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Vec<isobmff_tr::Transform>, Error> {
    let mut chain: Vec<isobmff_tr::Transform> = Vec::new();
    let mut cur = checked_image(heif, id, limits, budget)?;
    for _ in 0..=IDEN_DEPTH {
        let isobmff::ImageKind::Identity { input } = cur.kind else {
            break;
        };
        cur = checked_image(heif, input, limits, budget)?;
        let n = cur.transforms.len();
        budget.charge(n.saturating_mul(TR_SIZE))?;
        chain.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
        // This level lies below the ones already collected, so it goes first.
        chain.extend(cur.transforms.iter().map(to_tr));
        chain.rotate_right(n);
    }
    if matches!(cur.kind, isobmff::ImageKind::Identity { .. }) {
        return Err(Error::Unsupported("heif-identity-depth"));
    }
    if let isobmff::ImageKind::Grid(g) = &cur.kind {
        // `decode` counted the tiles with `check_ids` before this runs.
        for tile in &g.tiles {
            budget.check_cancel()?;
            if !heif.image(*tile)?.transforms.is_empty() {
                return Err(Error::Unsupported("grid-tile-transforms"));
            }
        }
    }
    Ok(chain)
}

fn plane_bytes(planes: &[Plane]) -> usize {
    planes.iter().fold(0usize, |n, p| {
        n.saturating_add(match &p.samples {
            Samples::U8(v) => v.len(),
            Samples::U16(v) => v.len().saturating_mul(2),
        })
    })
}

fn plane_bytes_tr(planes: &[isobmff_tr::Plane]) -> usize {
    planes.iter().fold(0usize, |n, p| {
        n.saturating_add(match &p.samples {
            isobmff_tr::PlaneSamples::U8(v) => v.len(),
            isobmff_tr::PlaneSamples::U16(v) => v.len().saturating_mul(2),
        })
    })
}

/// Applies `inner` (see [`inner_chain`]) and then `planes.transforms` to the planes. The sample
/// buffers are moved, not copied. The helper runs under what is left of `max_alloc`; the planes
/// it returns replace the input's charge.
pub(crate) fn apply(
    planes: Planes,
    mut chain: Vec<isobmff_tr::Transform>,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Planes, Error> {
    let Planes {
        width,
        height,
        chroma,
        planes,
        transforms,
        colour,
        cicp,
        metadata,
        transforms_applied: _,
    } = planes;
    let n = transforms.len();
    budget.charge(n.saturating_mul(TR_SIZE))?;
    chain.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
    chain.extend(transforms.iter().map(to_tr));
    let chain_bytes = chain.len().saturating_mul(TR_SIZE);
    let held = plane_bytes(&planes);
    let depths: Vec<u8> = planes.iter().map(|p| p.bit_depth).collect();
    let input = isobmff_tr::Planes {
        width,
        height,
        chroma: match chroma {
            Chroma::Mono => isobmff_tr::Chroma::Mono,
            Chroma::Yuv420 => isobmff_tr::Chroma::Yuv420,
            Chroma::Yuv422 => isobmff_tr::Chroma::Yuv422,
            Chroma::Yuv444 => isobmff_tr::Chroma::Yuv444,
        },
        planes: planes
            .into_iter()
            .map(|p| isobmff_tr::Plane {
                width: p.width,
                height: p.height,
                samples: match p.samples {
                    Samples::U8(v) => isobmff_tr::PlaneSamples::U8(v),
                    Samples::U16(v) => isobmff_tr::PlaneSamples::U16(v),
                },
            })
            .collect(),
    };
    let nested = isobmff_tr::Limits {
        max_alloc: budget.remaining(),
        max_threads: limits.max_threads,
        max_pixels: limits.max_pixels,
        ..isobmff_tr::Limits::default()
    };
    let out = isobmff_tr::apply_transforms(input, &chain, &nested, budget.cancel());
    budget.release(chain_bytes);
    let out = out?;
    if out.planes.len() != depths.len() {
        return Err(Error::Malformed("planes"));
    }
    // The helper ran under what was left, so its result fits once the input is given back.
    budget.release(held);
    budget.charge(plane_bytes_tr(&out.planes))?;
    Ok(Planes {
        width: out.width,
        height: out.height,
        chroma: match out.chroma {
            isobmff_tr::Chroma::Mono => Chroma::Mono,
            isobmff_tr::Chroma::Yuv420 => Chroma::Yuv420,
            isobmff_tr::Chroma::Yuv422 => Chroma::Yuv422,
            isobmff_tr::Chroma::Yuv444 => Chroma::Yuv444,
        },
        planes: out
            .planes
            .into_iter()
            .zip(depths)
            .map(|(p, bit_depth)| Plane {
                width: p.width,
                height: p.height,
                bit_depth,
                samples: match p.samples {
                    isobmff_tr::PlaneSamples::U8(v) => Samples::U8(v),
                    isobmff_tr::PlaneSamples::U16(v) => Samples::U16(v),
                },
            })
            .collect(),
        transforms,
        colour,
        cicp,
        metadata,
        transforms_applied: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn transforms_convert_field_by_field() {
        let c = isobmff::Clap {
            width_n: 1,
            width_d: 2,
            height_n: 3,
            height_d: 4,
            horiz_off_n: -5,
            horiz_off_d: 6,
            vert_off_n: 7,
            vert_off_d: 8,
        };
        assert_eq!(
            to_tr(&isobmff::Transform::Clap(c)),
            isobmff_tr::Transform::Clap(isobmff_tr::Clap {
                width_n: 1,
                width_d: 2,
                height_n: 3,
                height_d: 4,
                horiz_off_n: -5,
                horiz_off_d: 6,
                vert_off_n: 7,
                vert_off_d: 8,
            })
        );
        for deg in [0, 90, 180, 270] {
            assert_eq!(
                to_tr(&isobmff::Transform::Rotate(deg)),
                isobmff_tr::Transform::Rotate(deg)
            );
        }
        assert_eq!(
            to_tr(&isobmff::Transform::Mirror(isobmff::Axis::TopBottom)),
            isobmff_tr::Transform::Mirror(isobmff_tr::Axis::TopBottom)
        );
        assert_eq!(
            to_tr(&isobmff::Transform::Mirror(isobmff::Axis::LeftRight)),
            isobmff_tr::Transform::Mirror(isobmff_tr::Axis::LeftRight)
        );
    }

    #[test]
    fn helper_errors_map_to_ours() {
        use isobmff_tr::Error as E;
        assert_eq!(
            Error::from(E::LimitExceeded(isobmff_tr::Limit::Alloc)),
            Error::LimitExceeded(Limit::Alloc)
        );
        assert_eq!(
            Error::from(E::LimitExceeded(isobmff_tr::Limit::Pixels)),
            Error::LimitExceeded(Limit::Pixels)
        );
        assert_eq!(Error::from(E::OutOfMemory), Error::OutOfMemory);
        assert_eq!(Error::from(E::Cancelled), Error::Cancelled);
        assert_eq!(Error::from(E::Malformed("clap")), Error::Malformed("clap"));
        assert_eq!(Error::from(E::Truncated), Error::Truncated);
        assert_eq!(
            Error::from(E::LimitExceeded(isobmff_tr::Limit::Boxes)),
            Error::Malformed("transform")
        );
    }

    /// 4 x 2 luma, 4:2:2 chroma 2 x 2, 8-bit luma and 10-bit chroma.
    fn planes_422(transforms: Vec<isobmff::Transform>) -> Planes {
        let plane = |w, h, depth, s: Samples| Plane {
            width: w,
            height: h,
            bit_depth: depth,
            samples: s,
        };
        Planes {
            width: 4,
            height: 2,
            chroma: Chroma::Yuv422,
            planes: vec![
                plane(4, 2, 8, Samples::U8((0..8).collect())),
                plane(2, 2, 10, Samples::U16(vec![10, 11, 12, 13])),
                plane(2, 2, 10, Samples::U16(vec![20, 21, 22, 23])),
            ],
            transforms,
            colour: Vec::new(),
            cicp: None,
            metadata: Vec::new(),
            transforms_applied: false,
        }
    }

    #[test]
    fn apply_matches_the_helper_and_keeps_the_rest() {
        let list = vec![isobmff::Transform::Rotate(90)];
        let raw = planes_422(list.clone());
        let limits = Limits::default();
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&limits, &cancel);
        let out = apply(raw.clone(), Vec::new(), &limits, &budget).unwrap();

        // The same call on the helper's types.
        let tr = isobmff_tr::Planes {
            width: 4,
            height: 2,
            chroma: isobmff_tr::Chroma::Yuv422,
            planes: raw
                .planes
                .iter()
                .map(|p| isobmff_tr::Plane {
                    width: p.width,
                    height: p.height,
                    samples: match &p.samples {
                        Samples::U8(v) => isobmff_tr::PlaneSamples::U8(v.clone()),
                        Samples::U16(v) => isobmff_tr::PlaneSamples::U16(v.clone()),
                    },
                })
                .collect(),
        };
        let want = isobmff_tr::apply_transforms(
            tr,
            &[isobmff_tr::Transform::Rotate(90)],
            &isobmff_tr::Limits::default(),
            &cancel,
        )
        .unwrap();
        assert_eq!((out.width, out.height), (want.width, want.height));
        assert_eq!((out.width, out.height), (2, 4));
        // 4:2:2 rotated by 90 becomes 4:4:4.
        assert_eq!(out.chroma, Chroma::Yuv444);
        for (p, w) in out.planes.iter().zip(&want.planes) {
            assert_eq!((p.width, p.height), (w.width, w.height));
            match (&p.samples, &w.samples) {
                (Samples::U8(a), isobmff_tr::PlaneSamples::U8(b)) => assert_eq!(a, b),
                (Samples::U16(a), isobmff_tr::PlaneSamples::U16(b)) => assert_eq!(a, b),
                _ => panic!("sample type changed"),
            }
        }
        assert_eq!(
            out.planes.iter().map(|p| p.bit_depth).collect::<Vec<_>>(),
            [8, 10, 10]
        );
        // The list stays, the flag says it is applied.
        assert_eq!(out.transforms, list);
        assert!(out.transforms_applied);
        assert!(!raw.transforms_applied);
        // The ledger holds the result only: 2 x 4 luma bytes plus two 2 x 4 chroma planes of u16.
        assert_eq!(limits.max_alloc - budget.remaining(), 8 + 16 + 16);
    }

    #[test]
    fn inner_transforms_run_first_and_limits_hold() {
        let limits = Limits::default();
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&limits, &cancel);
        // Mirror (inner), then the item's own rotate 90: the reverse order gives [7, 3, 6, 2, 5, 1, 4, 0].
        let inner = vec![isobmff_tr::Transform::Mirror(isobmff_tr::Axis::LeftRight)];
        budget.charge(TR_SIZE).unwrap();
        let out = apply(
            planes_422(vec![isobmff::Transform::Rotate(90)]),
            inner,
            &limits,
            &budget,
        )
        .unwrap();
        let Samples::U8(luma) = &out.planes[0].samples else {
            panic!("luma is 8-bit")
        };
        assert_eq!(luma, &[0, 4, 1, 5, 2, 6, 3, 7]);
        assert_eq!(out.chroma, Chroma::Yuv444);
        assert_eq!((out.width, out.height), (2, 4));

        // The chain fits but the helper gets 39 bytes and needs 8 + 16 + 16: a limit error, not a panic.
        let small = Limits {
            max_alloc: TR_SIZE + 39,
            ..Limits::default()
        };
        let budget = Budget::new(&small, &cancel);
        let e = apply(
            planes_422(vec![isobmff::Transform::Rotate(90)]),
            Vec::new(),
            &small,
            &budget,
        )
        .unwrap_err();
        assert_eq!(e, Error::LimitExceeded(Limit::Alloc));
    }
}
