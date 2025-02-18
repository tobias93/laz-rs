use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::las::nir::Nir;
use crate::las::pointtypes::RGB;
use crate::las::{Point0, Point6};
use crate::LasZipError;

const DEFAULT_CHUNK_SIZE: usize = 50_000;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Version {
    major: u8,
    minor: u8,
    revision: u16,
}

impl Version {
    fn read_from<R: Read>(src: &mut R) -> std::io::Result<Self> {
        Ok(Self {
            major: src.read_u8()?,
            minor: src.read_u8()?,
            revision: src.read_u16::<LittleEndian>()?,
        })
    }

    fn write_to<W: Write>(&self, dst: &mut W) -> std::io::Result<()> {
        dst.write_u8(self.major)?;
        dst.write_u8(self.minor)?;
        dst.write_u16::<LittleEndian>(self.revision)?;
        Ok(())
    }
}

impl Default for Version {
    fn default() -> Self {
        Self {
            major: 2,
            minor: 2,
            revision: 0,
        }
    }
}

/// The different type of data / fields found in the definition of LAS points
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LazItemType {
    /// ExtraBytes for LAS versions <= 1.3 & point format <= 5
    Byte(u16),
    /// Point10 is the Point format id 0 of LAS for versions <= 1.3 & point format <= 5
    Point10,
    /// GpsTime for LAS versions <= 1.3 & point format <= 5
    GpsTime,
    /// RGB for LAS versions <= 1.3 & point format <= 5
    RGB12,
    //WavePacket13,
    /// Point14 is the Point format id 6 of LAS for versions >= 1.4 & point format >= 6
    Point14,
    /// RGB for LAS versions >= 1.4
    RGB14,
    /// RGB + Nir for LAS versions >= 1.4
    RGBNIR14,
    //WavePacket14,
    /// ExtraBytes for LAS versions >= 1.4
    Byte14(u16),
}

impl LazItemType {
    fn from_u16(item_type: u16, size: u16) -> Option<Self> {
        match item_type {
            0 => Some(LazItemType::Byte(size)),
            6 => Some(LazItemType::Point10),
            7 => Some(LazItemType::GpsTime),
            8 => Some(LazItemType::RGB12),
            //9 => LazItemType::WavePacket13,
            10 => Some(LazItemType::Point14),
            11 => Some(LazItemType::RGB14),
            12 => Some(LazItemType::RGBNIR14),
            //13 => LazItemType::WavePacket14,
            14 => Some(LazItemType::Byte14(size)),
            _ => None,
        }
    }

    fn size(&self) -> u16 {
        match self {
            LazItemType::Byte(size) => *size,
            LazItemType::Point10 => Point0::SIZE as u16,
            LazItemType::GpsTime => std::mem::size_of::<f64>() as u16,
            LazItemType::RGB12 => RGB::SIZE as u16,
            LazItemType::Point14 => Point6::SIZE as u16,
            LazItemType::RGB14 => RGB::SIZE as u16,
            LazItemType::RGBNIR14 => (RGB::SIZE + Nir::SIZE) as u16,
            LazItemType::Byte14(size) => *size,
        }
    }

    fn default_version(self) -> u16 {
        match self {
            LazItemType::Byte(_) => 2,
            LazItemType::Point10 => 2,
            LazItemType::GpsTime => 2,
            LazItemType::RGB12 => 2,
            LazItemType::Point14 => 3,
            LazItemType::RGB14 => 3,
            LazItemType::RGBNIR14 => 3,
            LazItemType::Byte14(_) => 3,
        }
    }
}

impl From<LazItemType> for u16 {
    fn from(t: LazItemType) -> Self {
        match t {
            LazItemType::Byte(_) => 0,
            LazItemType::Point10 => 6,
            LazItemType::GpsTime => 7,
            LazItemType::RGB12 => 8,
            //LazItemType::WavePacket13 => 9,
            LazItemType::Point14 => 10,
            LazItemType::RGB14 => 11,
            LazItemType::RGBNIR14 => 12,
            //LazItemType::WavePacket14 => 13,
            LazItemType::Byte14(_) => 14,
        }
    }
}

/// Struct stored as part of the laszip's vlr record_data
///
/// This gives information about the dimension compressed
/// and the version used for the compression.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct LazItem {
    // coded on a u16
    pub(crate) item_type: LazItemType,
    pub(crate) size: u16,
    pub(crate) version: u16,
}

impl LazItem {
    pub(crate) fn new(item_type: LazItemType, version: u16) -> Self {
        let size = item_type.size();
        Self {
            item_type,
            size,
            version,
        }
    }

    pub fn item_type(&self) -> LazItemType {
        self.item_type
    }

    pub fn size(&self) -> u16 {
        self.size
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    fn read_from<R: Read>(src: &mut R) -> crate::Result<Self> {
        let item_type = src.read_u16::<LittleEndian>()?;
        let size = src.read_u16::<LittleEndian>()?;
        let item_type = LazItemType::from_u16(item_type, size)
            .ok_or_else(|| LasZipError::UnknownLazItem(item_type))?;
        Ok(Self {
            item_type,
            size,
            version: src.read_u16::<LittleEndian>()?,
        })
    }

    fn write_to<W: Write>(&self, dst: &mut W) -> std::io::Result<()> {
        dst.write_u16::<LittleEndian>(self.item_type.into())?;
        dst.write_u16::<LittleEndian>(self.size)?;
        dst.write_u16::<LittleEndian>(self.version)?;
        Ok(())
    }
}

/// Defines a trait with one function to create
/// [LazItem]s to be used by a vlr.
///
/// The idea is that we define a different trait for each
/// version of the LAZ compression, plus one trait for the default version.
///
/// These traits only have one function where the implementers
/// must return the Laz Items for the LAZ version.
///
/// And we also have one struct for each possible point format, each
/// of this struct implements the traits depending of which point format
/// support which LAZ version + the default version trait.
macro_rules! define_trait_for_version {
    ($trait_name:ident, $trait_fn_name:ident) => {
        pub trait $trait_name {
            fn $trait_fn_name(num_extra_bytes: u16) -> Vec<LazItem>;
        }
    };
}

define_trait_for_version!(DefaultVersion, default_version);
define_trait_for_version!(Version1, version_1);
define_trait_for_version!(Version2, version_2);
define_trait_for_version!(Version3, version_3);

pub struct LazItemRecordBuilder {
    items: Vec<LazItemType>,
}

impl LazItemRecordBuilder {
    ///```
    /// let items = laz::LazItemRecordBuilder::default_version_of::<laz::las::Point0>(0);
    ///```
    pub fn default_version_of<PointFormat: DefaultVersion>(num_extra_bytes: u16) -> Vec<LazItem> {
        PointFormat::default_version(num_extra_bytes)
    }

    ///```
    /// let items = laz::LazItemRecordBuilder::version_1_of::<laz::las::Point0>(0);
    ///```
    ///
    /// ```compile_fail
    /// let items = laz::LazItemRecordBuilder::version_1_of::<laz::las::Point8>(0);
    /// ```
    pub fn version_1_of<PointFormat: Version1>(num_extra_bytes: u16) -> Vec<LazItem> {
        PointFormat::version_1(num_extra_bytes)
    }

    pub fn version_2_of<PointFormat: Version2>(num_extra_bytes: u16) -> Vec<LazItem> {
        PointFormat::version_2(num_extra_bytes)
    }

    pub fn version_3_of<PointFormat: Version3>(num_extra_bytes: u16) -> Vec<LazItem> {
        PointFormat::version_3(num_extra_bytes)
    }

    pub fn default_for_point_format_id(
        point_format_id: u8,
        num_extra_bytes: u16,
    ) -> crate::Result<Vec<LazItem>> {
        use crate::las::{Point1, Point2, Point3, Point7, Point8};
        match point_format_id {
            0 => Ok(LazItemRecordBuilder::default_version_of::<Point0>(
                num_extra_bytes,
            )),
            1 => Ok(LazItemRecordBuilder::default_version_of::<Point1>(
                num_extra_bytes,
            )),
            2 => Ok(LazItemRecordBuilder::default_version_of::<Point2>(
                num_extra_bytes,
            )),
            3 => Ok(LazItemRecordBuilder::default_version_of::<Point3>(
                num_extra_bytes,
            )),
            6 => Ok(LazItemRecordBuilder::default_version_of::<Point6>(
                num_extra_bytes,
            )),
            7 => Ok(LazItemRecordBuilder::default_version_of::<Point7>(
                num_extra_bytes,
            )),
            8 => Ok(LazItemRecordBuilder::default_version_of::<Point8>(
                num_extra_bytes,
            )),
            _ => Err(LasZipError::UnsupportedPointFormat(point_format_id)),
        }
    }

    pub fn new() -> Self {
        Self { items: vec![] }
    }

    pub fn add_item(&mut self, item_type: LazItemType) -> &mut Self {
        self.items.push(item_type);
        self
    }

    pub fn build(&self) -> Vec<LazItem> {
        self.items
            .iter()
            .map(|item_type| {
                let size = item_type.size();
                let version = item_type.default_version();
                LazItem {
                    item_type: *item_type,
                    size,
                    version,
                }
            })
            .collect()
    }
}

fn read_laz_items_from<R: Read>(mut src: &mut R) -> crate::Result<Vec<LazItem>> {
    let num_items = src.read_u16::<LittleEndian>()?;
    let mut items = Vec::<LazItem>::with_capacity(num_items as usize);
    for _ in 0..num_items {
        items.push(LazItem::read_from(&mut src)?)
    }
    Ok(items)
}

fn write_laz_items_to<W: Write>(laz_items: &Vec<LazItem>, mut dst: &mut W) -> std::io::Result<()> {
    dst.write_u16::<LittleEndian>(laz_items.len() as u16)?;
    for item in laz_items {
        item.write_to(&mut dst)?;
    }
    Ok(())
}

/// The possibilities for how the compressed data is organized.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CompressorType {
    None = 0,
    /// No chunks, or rather only 1 chunk with all the points
    PointWise = 1,
    /// Compress points into chunks with chunk_size points in each chunks
    PointWiseChunked = 2,
    /// Compress points into chunk, but also separate the different point dimension / fields
    /// into layers. This CompressorType is only use for point 6,7,8,9,10
    LayeredChunked = 3,
}

impl CompressorType {
    fn from_u16(t: u16) -> Option<Self> {
        match t {
            0 => Some(CompressorType::None),
            1 => Some(CompressorType::PointWise),
            2 => Some(CompressorType::PointWiseChunked),
            3 => Some(CompressorType::LayeredChunked),
            _ => None,
        }
    }

    fn from_item_version(item_version: u16) -> Option<Self> {
        match item_version {
            1 | 2 => Some(CompressorType::PointWiseChunked),
            3 | 4 => Some(CompressorType::LayeredChunked),
            _ => None,
        }
    }
}

impl Default for CompressorType {
    fn default() -> Self {
        CompressorType::PointWiseChunked
    }
}

/// The data stored in the record_data of the Laszip Vlr
///
/// This vlr contains information needed to compress or decompress
/// LAZ/LAS data. Such as the points per chunk, the fields & version
/// of the compression/decompression algorithm.
///
/// To create one from scratch, see the [`LazVlrBuilder`]
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LazVlr {
    // coded on u16
    pub(super) compressor: CompressorType,
    // 0 means ArithmeticCoder, its the only choice
    coder: u16,

    version: Version,
    options: u32,
    /// Number of points per chunk
    chunk_size: u32,

    // -1 if unused
    number_of_special_evlrs: i64,
    // -1 if unused
    offset_to_special_evlrs: i64,

    items: Vec<LazItem>,
}

impl LazVlr {
    /// The user id of the LasZip VLR header.
    pub const USER_ID: &'static str = "laszip encoded";
    /// The record id of the LasZip VLR header.
    pub const RECORD_ID: u16 = 22204;
    /// The description of the LasZip VLR header.
    pub const DESCRIPTION: &'static str = "https://laszip.org";
    // Sentinel value to indicate that chunks have a variable size.
    const VARIABLE_CHUNK_SIZE: u32 = u32::MAX;

    /// Creates a new LazVlr
    ///
    /// With **fixed-size** chunks.
    ///
    /// # panics
    ///
    /// Will panic if `items` is empty or contains invalid items.
    pub fn from_laz_items(items: Vec<LazItem>) -> Self {
        let first_item = items
            .first()
            .expect("Vec<LazItem> should at least have one element");
        let compressor = CompressorType::from_item_version(first_item.version)
            .expect("Unknown laz_item version");

        Self {
            compressor,
            coder: 0,
            version: Version::default(),
            options: 0,
            chunk_size: DEFAULT_CHUNK_SIZE as u32,
            number_of_special_evlrs: -1,
            offset_to_special_evlrs: -1,
            items,
        }
    }

    /// Tries to read the Vlr information from the record_data source
    pub fn read_from<R: Read>(mut src: R) -> crate::Result<Self> {
        let compressor_type = src.read_u16::<LittleEndian>()?;
        let compressor = match CompressorType::from_u16(compressor_type) {
            Some(c) => c,
            None => return Err(LasZipError::UnknownCompressorType(compressor_type)),
        };

        Ok(Self {
            compressor,
            coder: src.read_u16::<LittleEndian>()?,
            version: Version::read_from(&mut src)?,
            options: src.read_u32::<LittleEndian>()?,
            chunk_size: src.read_u32::<LittleEndian>()?,
            number_of_special_evlrs: src.read_i64::<LittleEndian>()?,
            offset_to_special_evlrs: src.read_i64::<LittleEndian>()?,
            items: read_laz_items_from(&mut src)?,
        })
    }

    pub fn from_buffer<T: AsRef<[u8]>>(buffer: T) -> crate::Result<Self> {
        Self::read_from(buffer.as_ref())
    }

    /// Writes the Vlr to the source.
    ///
    /// This **only** write the *record_data* the
    /// header should be written before-hand.
    pub fn write_to<W: Write>(&self, mut dst: &mut W) -> std::io::Result<()> {
        dst.write_u16::<LittleEndian>(self.compressor as u16)?;
        dst.write_u16::<LittleEndian>(self.coder)?;
        self.version.write_to(&mut dst)?;
        dst.write_u32::<LittleEndian>(self.options)?;
        dst.write_u32::<LittleEndian>(self.chunk_size)?;
        dst.write_i64::<LittleEndian>(self.number_of_special_evlrs)?;
        dst.write_i64::<LittleEndian>(self.offset_to_special_evlrs)?;
        write_laz_items_to(&self.items, &mut dst)?;
        Ok(())
    }

    #[inline]
    /// Returns whether the chunk size is variable.
    pub fn uses_variable_size_chunks(&self) -> bool {
        self.chunk_size == Self::VARIABLE_CHUNK_SIZE
    }

    /// Returns the chunk size, that is, the number of points in each chunk.
    ///
    /// This is only valid if [`Self::uses_variable_size_chunks`] returns false.
    #[inline]
    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    /// Returns the items compressed by this VLR
    #[inline]
    pub fn items(&self) -> &Vec<LazItem> {
        &self.items
    }

    /// Returns the sum of the size of the laz_items, which should correspond to the
    /// expected size of points (uncompressed).
    #[inline]
    pub fn items_size(&self) -> u64 {
        u64::from(self.items.iter().map(|item| item.size).sum::<u16>())
    }

    /// returns how many bytes a decompressed chunk contains
    #[cfg(feature = "parallel")]
    #[inline]
    pub(crate) fn num_bytes_in_decompressed_chunk(&self) -> u64 {
        self.chunk_size as u64 * self.items_size()
    }
}

/// Builder struct to personalize the LazVlr
///
/// # Examples
/// ```
/// # fn main() -> laz::Result<()> {
/// let vlr = laz::LazVlrBuilder::default()
///     .with_point_format(1, 0)?
///     .build();
/// # Ok(())
/// # }
/// ```
///
/// ```
/// # fn main() -> laz::Result<()> {
/// let vlr = laz::LazVlrBuilder::default()
///     .with_point_format(1, 0)?
///     .with_fixed_chunk_size(60_000)
///     .build();
/// # Ok(())
/// # }
/// ```
pub struct LazVlrBuilder {
    items: Vec<LazItem>,
    chunk_size: u32,
}

impl Default for LazVlrBuilder {
    fn default() -> Self {
        Self {
            items: vec![],
            chunk_size: DEFAULT_CHUNK_SIZE as u32,
        }
    }
}

impl LazVlrBuilder {
    pub fn new(laz_items: Vec<LazItem>) -> Self {
        Self {
            items: laz_items,
            ..Self::default()
        }
    }

    pub fn with_point_format(
        mut self,
        point_format_id: u8,
        num_extra_bytes: u16,
    ) -> crate::Result<Self> {
        self.items =
            LazItemRecordBuilder::default_for_point_format_id(point_format_id, num_extra_bytes)?;
        Ok(self)
    }

    pub fn with_laz_items(mut self, laz_items: Vec<LazItem>) -> Self {
        self.items = laz_items;
        self
    }

    pub fn with_fixed_chunk_size(mut self, chunk_size: u32) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn with_variable_chunk_size(mut self) -> Self {
        self.chunk_size = LazVlr::VARIABLE_CHUNK_SIZE;
        self
    }

    pub fn build(self) -> LazVlr {
        let mut vlr = LazVlr::from_laz_items(self.items);
        vlr.chunk_size = self.chunk_size;
        vlr
    }

    #[deprecated(
        since = "0.6.0",
        note = "Please use LazVlrBuilder::with_fixed_chunk_size"
    )]
    pub fn with_chunk_size(self, chunk_size: u32) -> Self {
        self.with_fixed_chunk_size(chunk_size)
    }

    #[deprecated(since = "0.6.0", note = "Please use LazVlrBuilder::new(laz_items)")]
    pub fn from_laz_items(laz_items: Vec<LazItem>) -> Self {
        Self::new(laz_items)
    }
}
