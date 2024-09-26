//! Multi-order Map, i.e. list of non-overlapping `(UNIQ, VALUE)`.
// IDEE:
// * use zuniq
// * use array of tuple, sorted according to zuniq
// * for each img pixel, get sky position, get zuniq, binay_search in map

use std::{
  cmp::Ordering,
  fmt::{Debug, Display},
  mem,
  iter::Map,
  slice::Iter,
};

use num::PrimInt;

use crate::nested::map::skymap::SkyMapValue;


/// `ZUniqHHashT` stands for `Z-curve ordered Uniq Healpix Hash Type`.
pub trait ZUniqHashT:
// 'static mean that Idx does not contains any reference
'static + PrimInt + Send + Sync + Debug + Display + Clone
{
  /// Number of unused bits (without including the sentinel bit).
  const N_RESERVED_BITS: u8 = 2;
  /// Number of bits needed to code a sub-cell relative index.
  const DIM: u8 = 2;
  /// Number of base cells, i.e. number of cell at depth 0.
  const N_D0_CELLS: u8 = 12;
  /// Number of bits needed to code the base cell index.
  const N_D0_BITS: u8 = 4;
  /// Number of bytes available in the 'ZUniqHHashT'.
  const N_BYTES: u8 = mem::size_of::<Self>() as u8;
  /// Number of bits available in the 'zuniq'.
  const N_BITS: u8 = Self::N_BYTES << 3;
  /// Maximum depth that can be coded on this 'ZUniqHHashT'.
  const MAX_DEPTH: u8 = (Self::N_BITS - (Self::N_RESERVED_BITS + Self::N_D0_BITS)) / Self::DIM;

  fn mult_by_dim<T: PrimInt>(v: T) -> T {
    v.unsigned_shl(1)
  }
  fn div_by_dim<T: PrimInt>(v: T) -> T {
    v.unsigned_shr(1)
  }
  fn shift(delta_depth: u8) -> u8 {
    Self::mult_by_dim(delta_depth)
  }
  fn delta_with_depth_max(depth: u8) -> u8 {
    Self::MAX_DEPTH - depth
  }
  fn shift_from_depth_max(depth: u8) -> u8 {
    Self::shift(Self::delta_with_depth_max(depth))
  }
  
  /// Returns the `depth` and `hash value` this `zuniq` contains.
  fn from_zuniq(zuniq: Self) -> (u8, Self) {
    let n_trailing_zero = zuniq.trailing_zeros() as u8;
    let delta_depth = Self::div_by_dim(n_trailing_zero);
    let depth = Self::MAX_DEPTH - delta_depth;
    let hash = zuniq >> (n_trailing_zero + 1) as usize;
    (depth, hash)
  }

  fn depth_from_zuniq(zuniq: Self) -> u8 {
    let n_trailing_zero = zuniq.trailing_zeros() as u8;
    let delta_depth = Self::div_by_dim(n_trailing_zero);
    let depth = Self::MAX_DEPTH - delta_depth;
    depth
  }

  /// Transforms a `depth` and `hash value` tuple into a `zuniq`.
  fn to_zuniq(depth: u8, hash: Self) -> Self {
    let zuniq = (hash << 1) | Self::one();
    zuniq.unsigned_shl(Self::shift_from_depth_max(depth) as u32)
  }

  fn are_overlapping(zuniq_l: Self, zuniq_r: Self) -> bool {
    let (depth_l, hash_l) = Self::from_zuniq(zuniq_l);
    let (depth_r, hash_r) = Self::from_zuniq(zuniq_r);
    Self::are_overlapping_cells(depth_l, hash_l, depth_r, hash_r)
  }
  
  fn are_overlapping_cells(depth_l: u8, hash_l: Self, depth_r: u8, hash_r: Self) -> bool {
    match depth_l.cmp(&depth_r) {
      Ordering::Equal   => hash_l == hash_r,
      Ordering::Less    => hash_l == hash_r.unsigned_shr(((depth_r - depth_l) << 1) as u32),
      Ordering::Greater => hash_r == hash_l.unsigned_shr(((depth_l - depth_r) << 1) as u32),
    }
  }

}

impl ZUniqHashT for u32 {}
impl ZUniqHashT for u64 {}

/// `MOM` stands for **M**ulti **O**rder healpix **M**aps.
/// Here, it consists in a list of HEALPix cells (hash) at various depth (HEALPixordes) 
/// with a value attached to each cell.
/// All cells in a given MOM:
/// * must be non-overlapping, 
/// * have the same type of value attached to them
/// * are sorted following the z-order curve order.
/// This last property allow for streaming processing and operations.
/// In practice, we use the `zuniq` index: it encodes both the depth and the cell hash value
/// and is built in such a way that the natural order follow the z-order curve order.  
pub trait Mom<'a> {
  /// Type of the HEALPix zuniq hash value (mainly `u32` or `u64`).
  type ZUniqHType: ZUniqHashT;
  /// Type of the iterator iterating on the skymap values.
  type ValueType: SkyMapValue + 'a;
  /// Type of iterator iterating on all (sorted!) zuniq values.
  type ZuniqIt: Iterator<Item = Self::ZUniqHType>;
  /// Type of iterator iterating on all (sorted!) entries.
  /// # Remark
  /// We could have defined `Iterator<Item = &'a (Self::ZUniqHType, Self::ValueType)>;`
  /// but it would have limited the possible implementations (e.g. using 2 vectors and the `zip` operation.
  type EntriesIt: Iterator<Item = (Self::ZUniqHType, &'a Self::ValueType)>;

  /// Largest depth the MOM may contain.
  fn depth_max(&self) -> u8;

  /// Returns the entry, if any, containing the given HEALPix cell hash computed at the `Mom`
  /// maximum depth.
  fn get_cell_containing(&'a self, zuniq_at_depth_max: Self::ZUniqHType) -> Result<Option<(Self::ZUniqHType, &'a Self::ValueType)>, String> {
    self.check_zuniq_depth_is_depth_max(zuniq_at_depth_max)
      .map(|_| self.get_cell_containing_unsafe(zuniq_at_depth_max))
  }

  /// Same as `get_cell_containing` without checking that `zuniq_at_depth_max` depth is the MOM
  /// maximum depth.
  fn get_cell_containing_unsafe(&'a self, hash_at_depth_max: Self::ZUniqHType) -> Option<(Self::ZUniqHType, &'a Self::ValueType)>;

  /// Returns all entries overlapped by the HEALPix cell of given `zuniq` hash value.
  fn get_overlapped_cells(&'a self, zuniq: Self::ZUniqHType) -> Vec<(Self::ZUniqHType, &'a Self::ValueType)>;

  /// Returns all HEALPix zuniq hash, ordered following the z-order curve.
  fn zuniqs(&'a self) -> Self::ZuniqIt;

  /// Returns all entries, i.e. HEALPix zuniq hash / value tuples, ordered following the z-order curve.
  fn entries(&'a self) -> Self::EntriesIt;

  /// Check if the gieven `zuniq` depth is the MOM maximum depth.
  fn check_zuniq_depth_is_depth_max(&self, zuniq_at_depth_max: Self::ZUniqHType) -> Result<(), String> {
    let depth = Self::ZUniqHType::depth_from_zuniq(zuniq_at_depth_max);
    if depth == self.depth_max() {
      Ok(())
    } else {
      Err(format!("Wrong depth for zuniq {}. Expected: {}. Actual: {}", zuniq_at_depth_max, self.depth_max(), depth))
    }
  }

  fn check_is_mom(&'a self) -> Result<(), String> {
    let mut it = self.zuniqs();
    if let Some(mut l) = it.next() {
      let (mut depth_l, mut hash_l) = Self::ZUniqHType::from_zuniq(l);
      for r in it {
        if depth_l < self.depth_max() {
          return Err(format!("Element has a larger depth than MOM maximum depth. Elem: {}; Depth: {}; Mom max depth: {}", l, depth_l, self.depth_max()));
        }
        let (depth_r, hash_r) =  Self::ZUniqHType::from_zuniq(r);
        if l >= r {
          return Err(format!("The MOM is not ordered: {} >= {}", l, r));
        } else if Self::ZUniqHType::are_overlapping_cells(depth_l, hash_l, depth_r, hash_r) {
          return Err(format!("Overlapping elements in the MOM: {} and {}. I.e. depth: {}; hash: {} and depth: {}, hash: {}.", l, r, depth_l, hash_l, depth_r, hash_r));
        }
        l = r;
        depth_l = depth_r;
        hash_l = hash_r;
      }
    }
    Ok(())
  }

}

/// Implementation of a MOM in a simple vector.
pub struct MomVecImpl<Z, V>
  where
    Z: ZUniqHashT,
    V: SkyMapValue
{
  depth: u8,
  entries: Vec<(Z, V)>,
}
impl<'a, Z, V> Mom<'a> for MomVecImpl<Z, V>
  where
    Z: ZUniqHashT,
    V: 'a + SkyMapValue
{
  type ZUniqHType = Z;
  type ValueType = V;
  type ZuniqIt = Map<Iter<'a, (Z, V)>, fn(&'a (Z, V)) -> Z>;
  type EntriesIt = Map<Iter<'a, (Z, V)>, fn(&'a (Z, V)) -> (Z, &'a V)>;

  fn depth_max(&self) -> u8 {
    self.depth
  }

  fn get_cell_containing_unsafe(&'a self, hash_at_depth_max: Self::ZUniqHType) -> Option<(Self::ZUniqHType, &'a Self::ValueType)> {
    match self.entries.binary_search_by(|&(z, _)| z.cmp(&hash_at_depth_max)) {
      Ok(i) => {
        let e = &self.entries[i];
        Some((e.0, &e.1))
      },
      Err(i) => {
        if i > 0 {
          // if array len is 0, i will be 0 so we do not enter here.
          let e = &self.entries[i - 1];
          if Z::are_overlapping(hash_at_depth_max, e.0) {
            return Some((e.0, &e.1));
          }
        }
        if i < self.entries.len() {
          let e = &self.entries[i];
          if Z::are_overlapping(hash_at_depth_max, e.0) {
            return Some((e.0, &e.1));
          }
        }
        None
      }
    }
  }

  fn get_overlapped_cells(&'a self, zuniq: Self::ZUniqHType) -> Vec<(Self::ZUniqHType, &'a Self::ValueType)> {
    let mut range = match self.entries.binary_search_by(|&(z, _)| z.cmp(&zuniq)) {
      Ok(i) => i..i + 1,
      Err(i) => i..i,
    };
    while range.start - 1 > 0 &&  Z::are_overlapping(zuniq, self.entries[range.start - 1].0) {
      range.start -= 1;
    }
    while range.end < self.entries.len() && Z::are_overlapping(zuniq, self.entries[range.end].0)  {
      range.end += 1;
    }
    range.into_iter().map(|i| {
      let (z, v) = &self.entries[i];
      (*z, v)
    }).collect()
  }

  fn zuniqs(&'a self) -> Self::ZuniqIt {
    self.entries.iter().map(|&(zuniq, _)| zuniq)
  }

  fn entries(&'a self) -> Self::EntriesIt {
    self.entries.iter().map(|(z, v)| (*z, v))
  }
}


/*

pub struct Mom {
  pub depth_max: u8,
  pub elems: MomElems,
}

pub enum FitsMomElems {
  U64U8(Vec<(u64, u8)>),
  U64I16(Vec<(u64, i16)>),
  U64I32(Vec<(u64, i32)>),
  U64I64(Vec<(u64, i64)>),
  U64F32(Vec<(u64, f32)>),
  U64F64(Vec<(u64, f64)>),
}

*/
