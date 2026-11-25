//! The opt-in `clap`, `irot` and `imir` step of [`decode_with`](crate::decode_with), through
//! `isobmff::apply_transforms` (docs/dependencies.md).

use crate::budget::Budget;
use crate::heif::{IDEN_DEPTH, checked_image};
use crate::{Chroma, Error, Limit, Limits, Plane, Planes, Samples};

/// The helper's limit, out-of-memory and cancel errors map to ours; `Malformed` and
/// `Unsupported` keep their token. It reports nothing else for planes; a variant it might add
/// is `Malformed("transform")`. Not `Error::from`, which wraps the rest as `Error::Heif`.
fn helper_error(e: isobmff::Error) -> Error {
    match e {
        isobmff::Error::LimitExceeded(isobmff::Limit::Alloc) => Error::LimitExceeded(Limit::Alloc),
        isobmff::Error::LimitExceeded(isobmff::Limit::Pixels) => {
            Error::LimitExceeded(Limit::Pixels)
        }
        isobmff::Error::OutOfMemory => Error::OutOfMemory,
        isobmff::Error::Cancelled => Error::Cancelled,
        isobmff::Error::Truncated => Error::Truncated,
        isobmff::Error::Malformed(what) => Error::Malformed(what),
        isobmff::Error::Unsupported(what) => Error::Unsupported(what),
        _ => Error::Malformed("transform"),
    }
}

const TR_SIZE: usize = size_of::<isobmff::Transform>();

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
) -> Result<Vec<isobmff::Transform>, Error> {
    let mut chain: Vec<isobmff::Transform> = Vec::new();
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
        chain.extend(cur.transforms.iter().cloned());
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

fn plane_bytes_tr(planes: &[isobmff::Plane]) -> usize {
    planes.iter().fold(0usize, |n, p| {
        n.saturating_add(match &p.samples {
            isobmff::PlaneSamples::U8(v) => v.len(),
            isobmff::PlaneSamples::U16(v) => v.len().saturating_mul(2),
        })
    })
}

/// Applies `inner` (see [`inner_chain`]) and then `planes.transforms` to the planes. The sample
/// buffers are moved, not copied. The helper runs under what is left of `max_alloc`; the planes
/// it returns replace the input's charge.
pub(crate) fn apply(
    planes: Planes,
    mut chain: Vec<isobmff::Transform>,
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
    chain.extend(transforms.iter().cloned());
    let chain_bytes = chain.len().saturating_mul(TR_SIZE);
    let held = plane_bytes(&planes);
    let depths: Vec<u8> = planes.iter().map(|p| p.bit_depth).collect();
    let input = isobmff::Planes {
        width,
        height,
        chroma: match chroma {
            Chroma::Mono => isobmff::Chroma::Mono,
            Chroma::Yuv420 => isobmff::Chroma::Yuv420,
            Chroma::Yuv422 => isobmff::Chroma::Yuv422,
            Chroma::Yuv444 => isobmff::Chroma::Yuv444,
        },
        planes: planes
            .into_iter()
            .map(|p| isobmff::Plane {
                width: p.width,
                height: p.height,
                samples: match p.samples {
                    Samples::U8(v) => isobmff::PlaneSamples::U8(v),
                    Samples::U16(v) => isobmff::PlaneSamples::U16(v),
                },
            })
            .collect(),
    };
    let nested = isobmff::Limits {
        max_alloc: budget.remaining(),
        max_threads: limits.max_threads,
        max_pixels: limits.max_pixels,
        ..isobmff::Limits::default()
    };
    let out = isobmff::apply_transforms(input, &chain, &nested, budget.cancel());
    budget.release(chain_bytes);
    let out = out.map_err(helper_error)?;
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
            isobmff::Chroma::Mono => Chroma::Mono,
            isobmff::Chroma::Yuv420 => Chroma::Yuv420,
            isobmff::Chroma::Yuv422 => Chroma::Yuv422,
            isobmff::Chroma::Yuv444 => Chroma::Yuv444,
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
                    isobmff::PlaneSamples::U8(v) => Samples::U8(v),
                    isobmff::PlaneSamples::U16(v) => Samples::U16(v),
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

    type Item<'a> = (u16, &'a [u8; 4], &'a [u8], &'a [u8]);

    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        cat(&[&((body.len() + 8) as u32).to_be_bytes(), kind, body])
    }

    fn fbx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        bx(kind, &cat(&[&[0; 4], body]))
    }

    fn ispe(w: u32, h: u32) -> Vec<u8> {
        fbx(b"ispe", &cat(&[&w.to_be_bytes(), &h.to_be_bytes()]))
    }

    /// A HEIF file of `(id, type, data, 1-based property indices)` items (the primary item is 1),
    /// the `ipco` properties and `dimg` references `(from, to)`: `iloc` version 0, one extent each.
    fn heif_bytes(items: &[Item<'_>], props: &[Vec<u8>], dimg: &[(u16, &[u16])]) -> Vec<u8> {
        let ftyp = bx(b"ftyp", b"avif\0\0\0\0mif1avif");
        let meta = |mdat_at: u32| {
            let mut kids = fbx(b"hdlr", &cat(&[&[0; 4], b"pict", &[0; 13]]));
            kids.extend(fbx(b"pitm", &1u16.to_be_bytes()));
            let mut infes = Vec::new();
            for (id, kind, _, _) in items {
                let b = cat(&[&id.to_be_bytes(), &[0, 0], *kind, &[0]]);
                infes.extend(bx(b"infe", &cat(&[&[2, 0, 0, 0], &b])));
            }
            kids.extend(fbx(
                b"iinf",
                &cat(&[&(items.len() as u16).to_be_bytes(), &infes]),
            ));
            let mut iloc = vec![0x44, 0];
            iloc.extend_from_slice(&(items.len() as u16).to_be_bytes());
            let mut at = mdat_at;
            for (id, _, data, _) in items {
                iloc.extend_from_slice(&id.to_be_bytes());
                iloc.extend_from_slice(&[0, 0, 0, 1]);
                iloc.extend_from_slice(&at.to_be_bytes());
                iloc.extend_from_slice(&(data.len() as u32).to_be_bytes());
                at += data.len() as u32;
            }
            kids.extend(fbx(b"iloc", &iloc));
            let mut refs = Vec::new();
            for (from, to) in dimg {
                let mut b = cat(&[&from.to_be_bytes(), &(to.len() as u16).to_be_bytes()]);
                for t in *to {
                    b.extend_from_slice(&t.to_be_bytes());
                }
                refs.extend(bx(b"dimg", &b));
            }
            kids.extend(fbx(b"iref", &refs));
            let mut ipma = (items.len() as u32).to_be_bytes().to_vec();
            for (id, _, _, assoc) in items {
                ipma.extend_from_slice(&id.to_be_bytes());
                ipma.push(assoc.len() as u8);
                // Every association is essential.
                ipma.extend(assoc.iter().map(|i| 0x80 | i));
            }
            let ipco = bx(b"ipco", &props.concat());
            kids.extend(bx(b"iprp", &cat(&[&ipco, &fbx(b"ipma", &ipma)])));
            fbx(b"meta", &kids)
        };
        let mdat_at = (ftyp.len() + meta(0).len() + 8) as u32;
        let data: Vec<u8> = items.iter().flat_map(|i| i.2.to_vec()).collect();
        cat(&[&ftyp, &meta(mdat_at), &bx(b"mdat", &data)])
    }

    #[test]
    fn inner_chain_is_innermost_first_and_grid_tiles_add_nothing() {
        use isobmff::Transform::Rotate;
        let limits = Limits::default();
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&limits, &cancel);
        // `irot` 1, 2 and 3 are rotations by 90, 180 and 270 degrees.
        let props = [
            ispe(64, 48),
            bx(b"av1C", &[0x81, 0x0c, 0x4c, 0x00, 0x0a, 0x00]),
            bx(b"irot", &[1]),
            bx(b"irot", &[2]),
            bx(b"irot", &[3]),
        ];
        // iden 3 [irot 270] -> iden 2 [irot 180] -> av01 1 [irot 90]; grid 5 [irot 180] of tile 4.
        let grid = [0u8, 0, 0, 0, 0, 64, 0, 48];
        let items: [Item<'_>; 5] = [
            (1, b"av01", &[1; 4], &[1, 2, 3]),
            (2, b"iden", &[], &[1, 4]),
            (3, b"iden", &[], &[1, 5]),
            (4, b"av01", &[4; 4], &[1, 2]),
            (5, b"grid", &grid, &[1, 4]),
        ];
        let file = heif_bytes(&items, &props, &[(2, &[1]), (3, &[2]), (5, &[4])]);
        let heif = isobmff::parse(&file, &isobmff::Limits::default(), &cancel).unwrap();

        // The item's own list (270) is not in the chain: `apply` adds it last.
        let chain = inner_chain(&heif, 3, &limits, &budget).unwrap();
        assert_eq!(chain, [Rotate(90), Rotate(180)]);
        assert_eq!(limits.max_alloc - budget.remaining(), 2 * TR_SIZE);
        // Not an `iden`: nothing.
        assert!(inner_chain(&heif, 1, &limits, &budget).unwrap().is_empty());
        // A grid whose tile has no transforms of its own: nothing (its own list is `apply`'s).
        assert!(inner_chain(&heif, 5, &limits, &budget).unwrap().is_empty());
    }

    #[test]
    fn helper_errors_map_to_ours() {
        use isobmff::Error as E;
        assert_eq!(
            helper_error(E::LimitExceeded(isobmff::Limit::Alloc)),
            Error::LimitExceeded(Limit::Alloc)
        );
        assert_eq!(
            helper_error(E::LimitExceeded(isobmff::Limit::Pixels)),
            Error::LimitExceeded(Limit::Pixels)
        );
        assert_eq!(helper_error(E::OutOfMemory), Error::OutOfMemory);
        assert_eq!(helper_error(E::Cancelled), Error::Cancelled);
        assert_eq!(helper_error(E::Malformed("clap")), Error::Malformed("clap"));
        assert_eq!(helper_error(E::Truncated), Error::Truncated);
        assert_eq!(
            helper_error(E::LimitExceeded(isobmff::Limit::Boxes)),
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
        // As in `decode`, the input planes are charged before the step.
        budget.charge(plane_bytes(&raw.planes)).unwrap();
        let out = apply(raw.clone(), Vec::new(), &limits, &budget).unwrap();

        // The same call on the helper's types.
        let tr = isobmff::Planes {
            width: 4,
            height: 2,
            chroma: isobmff::Chroma::Yuv422,
            planes: raw
                .planes
                .iter()
                .map(|p| isobmff::Plane {
                    width: p.width,
                    height: p.height,
                    samples: match &p.samples {
                        Samples::U8(v) => isobmff::PlaneSamples::U8(v.clone()),
                        Samples::U16(v) => isobmff::PlaneSamples::U16(v.clone()),
                    },
                })
                .collect(),
        };
        let want = isobmff::apply_transforms(
            tr,
            &[isobmff::Transform::Rotate(90)],
            &isobmff::Limits::default(),
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
                (Samples::U8(a), isobmff::PlaneSamples::U8(b)) => assert_eq!(a, b),
                (Samples::U16(a), isobmff::PlaneSamples::U16(b)) => assert_eq!(a, b),
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
        let inner = vec![isobmff::Transform::Mirror(isobmff::Axis::LeftRight)];
        budget.charge(TR_SIZE).unwrap();
        budget
            .charge(plane_bytes(&planes_422(Vec::new()).planes))
            .unwrap();
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

        // The input (24 bytes, charged) and the chain fit but the helper gets 39 bytes and needs 8 + 16 + 16:
        // a limit error, not a panic.
        let small = Limits {
            max_alloc: TR_SIZE + 24 + 39,
            ..Limits::default()
        };
        let budget = Budget::new(&small, &cancel);
        budget
            .charge(plane_bytes(&planes_422(Vec::new()).planes))
            .unwrap();
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
