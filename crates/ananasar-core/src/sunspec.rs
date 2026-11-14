//! SunSpec device maps: the point types, their encoding and the walk over a
//! device's models, following the SunSpec Information Model Specification.
//!
//! A SunSpec device starts with the marker `SunS` (0x5375 0x6E53) at one of
//! three well-known base addresses, followed by a chain of models. Each model
//! is a 2-register header (model ID, then length L of the data that follows in
//! registers) and its data; the chain ends with model ID 0xFFFF. Every value
//! is big-endian (most significant word first, high byte first), integers
//! carry a "not implemented" value outside their valid range, and integers
//! may be scaled by a power of ten held in a `sunssf` point.
//!
//! This module is pure: model definitions come in as data (the GUI reads them
//! from SunSpec's JSON model files) and registers are read through a closure.

use crate::typed::WordOrder;
use std::fmt;

/// The wire addresses where a SunSpec map may start, in the order a reader
/// tries them. The specification names them as 1-based register numbers
/// 40001, 00001 and 50001.
pub const BASE_ADDRESSES: [u16; 3] = [40000, 0, 50000];

/// The two registers of the `SunS` marker.
pub const MARKER: [u16; 2] = [0x5375, 0x6E53];

/// The model ID that ends a device's chain of models.
pub const END_MODEL: u16 = 0xFFFF;

/// A SunSpec point type, as named in the model files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs, reason = "the variants are SunSpec's own type names")]
pub enum PointType {
    Int16,
    Uint16,
    Count,
    Acc16,
    Enum16,
    Bitfield16,
    Pad,
    Sunssf,
    Raw16,
    Int32,
    Uint32,
    Acc32,
    Enum32,
    Bitfield32,
    Ipaddr,
    Float32,
    Int64,
    Uint64,
    Acc64,
    Bitfield64,
    Float64,
    Eui48,
    Ipv6addr,
    String,
}

impl PointType {
    /// The type for a model file's `type` name, or `None` for an unknown one.
    pub fn from_name(name: &str) -> Option<Self> {
        use PointType as T;
        Some(match name {
            "int16" => T::Int16,
            "uint16" => T::Uint16,
            "count" => T::Count,
            "acc16" => T::Acc16,
            "enum16" => T::Enum16,
            "bitfield16" => T::Bitfield16,
            "pad" => T::Pad,
            "sunssf" => T::Sunssf,
            "raw16" => T::Raw16,
            "int32" => T::Int32,
            "uint32" => T::Uint32,
            "acc32" => T::Acc32,
            "enum32" => T::Enum32,
            "bitfield32" => T::Bitfield32,
            "ipaddr" => T::Ipaddr,
            "float32" => T::Float32,
            "int64" => T::Int64,
            "uint64" => T::Uint64,
            "acc64" => T::Acc64,
            "bitfield64" => T::Bitfield64,
            "float64" => T::Float64,
            "eui48" => T::Eui48,
            "ipv6addr" => T::Ipv6addr,
            "string" => T::String,
            _ => return None,
        })
    }

    /// The size in registers, fixed for every type but `string`, whose size
    /// the model gives.
    pub fn fixed_size(self) -> Option<u16> {
        use PointType as T;
        match self {
            T::Int16 | T::Uint16 | T::Count | T::Acc16 | T::Enum16 | T::Bitfield16 | T::Pad | T::Sunssf | T::Raw16 => {
                Some(1)
            }
            T::Int32 | T::Uint32 | T::Acc32 | T::Enum32 | T::Bitfield32 | T::Ipaddr | T::Float32 => Some(2),
            T::Int64 | T::Uint64 | T::Acc64 | T::Bitfield64 | T::Float64 | T::Eui48 => Some(4),
            T::Ipv6addr => Some(8),
            T::String => None,
        }
    }
}

/// Where a point's scale factor comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScaleFactor {
    /// A power of ten given in the model.
    Fixed(i8),
    /// The name of a `sunssf` point in the same group or an enclosing one.
    Point(String),
}

/// A name for one value of an enum, or for one bit of a bitfield.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// The symbol's name, such as `MPPT`.
    pub name: String,
    /// The enum value, or the bit number for a bitfield.
    pub value: u64,
}

/// One point of a model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Point {
    /// The point's name, such as `W`.
    pub name: String,
    /// Its type.
    pub ty: PointType,
    /// Its size in registers.
    pub size: u16,
    /// Its scale factor, if it has one.
    pub sf: Option<ScaleFactor>,
    /// Its units, such as `W`, as free text.
    pub units: Option<String>,
    /// Names for enum values or bitfield bits.
    pub symbols: Vec<Symbol>,
}

/// How many times a group repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupCount {
    /// A fixed number of instances.
    Fixed(u16),
    /// As many instances as the model's length leaves room for.
    FromLength,
}

/// A group of points, possibly repeated, possibly holding further groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The group's name.
    pub name: String,
    /// How often it repeats; the model's top group is always `Fixed(1)`.
    pub count: GroupCount,
    /// Its points, in register order.
    pub points: Vec<Point>,
    /// Its child groups, which follow the points.
    pub groups: Vec<Group>,
}

/// A model definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    /// The model ID.
    pub id: u16,
    /// A human-readable name, if the model file gives one.
    pub label: Option<String>,
    /// The top group; its first two points are the ID and L header.
    pub group: Group,
}

/// A model found on a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelHeader {
    /// The model ID.
    pub id: u16,
    /// The address of its ID register.
    pub addr: u16,
    /// Its length L: the registers after the ID and L header.
    pub len: u16,
}

/// The SunSpec map of a device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The address of the `SunS` marker.
    pub base: u16,
    /// The models in the chain, in order.
    pub models: Vec<ModelHeader>,
    /// Whether the chain ended with the end model; false when it ran off the
    /// end of the bank.
    pub complete: bool,
}

/// Find a SunSpec map through `read`, which returns the register at an
/// address. Tries each base address in turn.
pub fn discover(read: impl Fn(u16) -> u16) -> Option<Device> {
    let base =
        BASE_ADDRESSES.into_iter().find(|&b| b.checked_add(1).is_some_and(|next| [read(b), read(next)] == MARKER))?;
    let mut models = Vec::new();
    // Every step moves at least two registers forward, so the walk ends at the
    // end model or when the next address would pass the end of the bank.
    let mut next = base.checked_add(2);
    while let Some(addr) = next {
        let id = read(addr);
        if id == END_MODEL {
            return Some(Device { base, models, complete: true });
        }
        let Some(len_addr) = addr.checked_add(1) else { break };
        let len = read(len_addr);
        models.push(ModelHeader { id, addr, len });
        next = len_addr.checked_add(1).and_then(|data| data.checked_add(len));
    }
    Some(Device { base, models, complete: false })
}

/// A point placed at its address in one model instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed<'a> {
    /// The point's name with its group path, such as `module[2].DCA`.
    pub path: String,
    /// The point definition.
    pub point: &'a Point,
    /// The address of its first register.
    pub addr: u16,
    /// Where its scale factor comes from, resolved.
    pub sf: Option<ResolvedSf>,
}

/// A scale factor after the model has been laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedSf {
    /// A fixed power of ten.
    Fixed(i8),
    /// The `sunssf` register at this address.
    At(u16),
    /// The model names a scale factor point that it does not contain.
    Missing,
}

fn points_size(points: &[Point]) -> Option<u16> {
    points.iter().try_fold(0u16, |n, p| n.checked_add(p.size))
}

/// The size of one instance of `group`, counting child groups with a fixed
/// count; `None` if a child's count depends on the model length.
fn instance_size(group: &Group) -> Option<u16> {
    group.groups.iter().try_fold(points_size(&group.points)?, |n, g| match g.count {
        GroupCount::Fixed(c) => n.checked_add(instance_size(g)?.checked_mul(c)?),
        GroupCount::FromLength => None,
    })
}

/// Lay `model` out at the header `at`: every point with its address, the
/// repeating groups expanded. Points that would fall outside the model's
/// length or past the end of the bank are left out.
pub fn layout<'a>(model: &'a Model, at: ModelHeader) -> Vec<Placed<'a>> {
    let mut out = Vec::new();
    // The model's data ends L registers after the 2-register header.
    let end = u32::from(at.addr).saturating_add(2).saturating_add(u32::from(at.len));
    let mut scopes: Vec<Vec<(&'a str, u16)>> = Vec::new();
    place_group(&model.group, "", u32::from(at.addr), end, &mut scopes, &mut out);
    out
}

/// Place one instance of `group` starting at `start`; returns where it ends.
fn place_group<'a>(
    group: &'a Group,
    prefix: &str,
    start: u32,
    end: u32,
    scopes: &mut Vec<Vec<(&'a str, u16)>>,
    out: &mut Vec<Placed<'a>>,
) -> u32 {
    let mut addr = start;
    let mut here = Vec::new();
    let mut placed = Vec::new();
    for p in &group.points {
        let next = addr.saturating_add(u32::from(p.size));
        if let Ok(a) = u16::try_from(addr)
            && next <= end
        {
            here.push((p.name.as_str(), a));
            placed.push((p, a));
        }
        addr = next;
    }
    scopes.push(here);
    for (p, a) in placed {
        let sf = p.sf.as_ref().map(|sf| match sf {
            ScaleFactor::Fixed(n) => ResolvedSf::Fixed(*n),
            ScaleFactor::Point(name) => scopes
                .iter()
                .rev()
                .flat_map(|s| s.iter())
                .find(|(n, _)| n == name)
                .map_or(ResolvedSf::Missing, |&(_, a)| ResolvedSf::At(a)),
        });
        out.push(Placed { path: format!("{prefix}{}", p.name), point: p, addr: a, sf });
    }
    for child in &group.groups {
        let count = match child.count {
            GroupCount::Fixed(c) => u32::from(c),
            // The instances fill what the model length leaves after the fixed
            // part; the specification says readers must compute this rather
            // than trust a count point.
            GroupCount::FromLength => match instance_size(child) {
                Some(size) if size > 0 => end.saturating_sub(addr).checked_div(u32::from(size)).unwrap_or(0),
                _ => 0,
            },
        };
        for i in 0..count {
            let name = format!("{prefix}{}[{}].", child.name, i.saturating_add(1));
            addr = place_group(child, &name, addr, end, scopes, out);
        }
    }
    scopes.pop();
    addr
}

/// A decimal number: `raw` × 10^`exp`, kept exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decimal {
    /// The integer as read from the registers.
    pub raw: i128,
    /// The power of ten it is scaled by.
    pub exp: i8,
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.raw < 0 { "-" } else { "" };
        let mut digits = self.raw.unsigned_abs().to_string();
        let shift = usize::from(self.exp.unsigned_abs());
        if self.exp >= 0 {
            if self.raw != 0 {
                digits.extend(std::iter::repeat_n('0', shift));
            }
            return write!(f, "{sign}{digits}");
        }
        if digits.len() <= shift {
            let pad = shift.saturating_sub(digits.len()).saturating_add(1);
            digits.insert_str(0, &"0".repeat(pad));
        }
        let (int, frac) = digits.split_at(digits.len().saturating_sub(shift));
        write!(f, "{sign}{int}.{frac}")
    }
}

/// A decoded point value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// The device marks the point, or its scale factor, as not implemented.
    NotImplemented,
    /// A reserved register.
    Pad,
    /// An integer, scaled by its scale factor.
    Number(Decimal),
    /// A floating-point value.
    Float(f64),
    /// An enum value and its symbol name, if the model has one.
    Enum(u64, Option<String>),
    /// A bitfield and the names of its set bits.
    Bits(u64, Vec<String>),
    /// Text: a string, or an IP or EUI-48 address.
    Text(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::NotImplemented => f.write_str("n/a"),
            Value::Pad => f.write_str(""),
            Value::Number(d) => write!(f, "{d}"),
            Value::Float(v) => write!(f, "{v}"),
            Value::Enum(v, Some(name)) => write!(f, "{name} ({v})"),
            Value::Enum(v, None) => write!(f, "{v}"),
            Value::Bits(v, names) if names.is_empty() => write!(f, "0x{v:X}"),
            Value::Bits(v, names) => write!(f, "{} (0x{v:X})", names.join(", ")),
            Value::Text(s) => f.write_str(s),
        }
    }
}

fn regs_n<const N: usize>(regs: &[u16]) -> Option<[u16; N]> {
    regs.get(..N)?.try_into().ok()
}

/// The registers of a 64-bit point as an unsigned integer.
fn u64_of(regs: &[u16]) -> Option<u64> {
    Some(WordOrder::Abcd.decode64(regs_n(regs)?).u)
}

/// The scale factor read from a `sunssf` register: `None` when it is not
/// implemented (0x8000) or outside the range −10…10 the specification allows.
pub fn scale_factor(reg: u16) -> Option<i8> {
    let sf = i16::from_be_bytes(reg.to_be_bytes());
    if (-10..=10).contains(&sf) { i8::try_from(sf).ok() } else { None }
}

/// Decode `point` from its registers `regs`. `sf` is its scale factor, if it
/// has one: `Some(Some(n))` for 10^n, `Some(None)` when the scale factor is
/// not implemented, `None` when the point has none. Returns `None` when
/// `regs` is shorter than the point.
pub fn decode(point: &Point, regs: &[u16], sf: Option<Option<i8>>) -> Option<Value> {
    use PointType as T;
    let size = usize::from(point.size);
    let regs = regs.get(..size)?;
    let number = |raw: i128, not_impl: bool| match (not_impl, sf) {
        (true, _) | (_, Some(None)) => Value::NotImplemented,
        (false, Some(Some(exp))) => Value::Number(Decimal { raw, exp }),
        (false, None) => Value::Number(Decimal { raw, exp: 0 }),
    };
    Some(match point.ty {
        T::Pad => Value::Pad,
        T::Int16 | T::Sunssf => {
            let [r] = regs_n(regs)?;
            number(i16::from_be_bytes(r.to_be_bytes()).into(), r == 0x8000)
        }
        T::Uint16 | T::Count => {
            let [r] = regs_n(regs)?;
            number(r.into(), r == 0xFFFF)
        }
        T::Acc16 | T::Raw16 => {
            let [r] = regs_n(regs)?;
            number(r.into(), false)
        }
        T::Int32 => {
            let v = WordOrder::Abcd.decode32(regs_n(regs)?);
            number(v.i.into(), v.u == 0x8000_0000)
        }
        T::Uint32 => {
            let v = WordOrder::Abcd.decode32(regs_n(regs)?).u;
            number(v.into(), v == 0xFFFF_FFFF)
        }
        T::Acc32 => number(WordOrder::Abcd.decode32(regs_n(regs)?).u.into(), false),
        T::Int64 => {
            let v = WordOrder::Abcd.decode64(regs_n(regs)?);
            number(v.i.into(), v.u == 0x8000_0000_0000_0000)
        }
        T::Uint64 => {
            let v = u64_of(regs)?;
            number(v.into(), v == u64::MAX)
        }
        T::Acc64 => number(u64_of(regs)?.into(), false),
        T::Float32 => {
            let v = WordOrder::Abcd.decode32(regs_n(regs)?).f;
            if v.is_nan() { Value::NotImplemented } else { Value::Float(v.into()) }
        }
        T::Float64 => {
            let v = WordOrder::Abcd.decode64(regs_n(regs)?).f;
            if v.is_nan() { Value::NotImplemented } else { Value::Float(v) }
        }
        T::Enum16 | T::Enum32 => {
            let (v, not_impl) = if point.ty == T::Enum16 {
                let [r] = regs_n(regs)?;
                (u64::from(r), r == 0xFFFF)
            } else {
                let v = WordOrder::Abcd.decode32(regs_n(regs)?).u;
                (u64::from(v), v == 0xFFFF_FFFF)
            };
            if not_impl {
                Value::NotImplemented
            } else {
                Value::Enum(v, point.symbols.iter().find(|s| s.value == v).map(|s| s.name.clone()))
            }
        }
        T::Bitfield16 | T::Bitfield32 | T::Bitfield64 => {
            let (v, top) = match point.ty {
                T::Bitfield16 => (u64::from(regs_n::<1>(regs)?.first().copied()?), 1u64 << 15),
                T::Bitfield32 => (u64::from(WordOrder::Abcd.decode32(regs_n(regs)?).u), 1u64 << 31),
                _ => (u64_of(regs)?, 1u64 << 63),
            };
            // "If the most significant bit in a bitfield is set, all other
            // bits shall be ignored": the value is not usable.
            if v & top != 0 {
                Value::NotImplemented
            } else {
                let set = |bit: u64| u32::try_from(bit).ok().and_then(|b| v.checked_shr(b)).is_some_and(|s| s & 1 == 1);
                Value::Bits(v, point.symbols.iter().filter(|s| set(s.value)).map(|s| s.name.clone()).collect())
            }
        }
        T::String => {
            let bytes: Vec<u8> = regs.iter().flat_map(|r| r.to_be_bytes()).collect();
            if bytes.iter().all(|&b| b == 0) {
                Value::NotImplemented
            } else {
                let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i.saturating_add(1));
                Value::Text(String::from_utf8_lossy(bytes.get(..end).unwrap_or_default()).into_owned())
            }
        }
        T::Ipaddr => {
            let v = WordOrder::Abcd.decode32(regs_n(regs)?).u;
            Value::Text(std::net::Ipv4Addr::from(v).to_string())
        }
        T::Ipv6addr => {
            let bytes: Vec<u8> = regs.iter().flat_map(|r| r.to_be_bytes()).collect();
            let octets: [u8; 16] = bytes.try_into().ok()?;
            Value::Text(std::net::Ipv6Addr::from(octets).to_string())
        }
        T::Eui48 => {
            // Four registers; the first two bytes are unused.
            let bytes = u64_of(regs)?.to_be_bytes();
            let mac = bytes.get(2..)?.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
            Value::Text(mac)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(name: &str, ty: &str, size: u16) -> Point {
        Point {
            name: name.into(),
            ty: PointType::from_name(ty).unwrap(),
            size,
            sf: None,
            units: None,
            symbols: Vec::new(),
        }
    }

    fn with_sf(mut p: Point, sf: &str) -> Point {
        p.sf = Some(ScaleFactor::Point(sf.into()));
        p
    }

    fn sym(name: &str, value: u64) -> Symbol {
        Symbol { name: name.into(), value }
    }

    fn dec(ty: &str, size: u16, regs: &[u16]) -> String {
        decode(&pt("x", ty, size), regs, None).unwrap().to_string()
    }

    #[test]
    fn sizes_follow_the_types() {
        assert_eq!(PointType::from_name("int16").unwrap().fixed_size(), Some(1));
        assert_eq!(PointType::from_name("acc32").unwrap().fixed_size(), Some(2));
        assert_eq!(PointType::from_name("eui48").unwrap().fixed_size(), Some(4));
        assert_eq!(PointType::from_name("ipv6addr").unwrap().fixed_size(), Some(8));
        assert_eq!(PointType::from_name("string").unwrap().fixed_size(), None);
        assert_eq!(PointType::from_name("int128"), None);
    }

    #[test]
    fn integers_and_their_not_implemented_values() {
        assert_eq!(dec("int16", 1, &[0xFFFE]), "-2");
        assert_eq!(dec("int16", 1, &[0x8000]), "n/a");
        assert_eq!(dec("uint16", 1, &[65534]), "65534");
        assert_eq!(dec("uint16", 1, &[0xFFFF]), "n/a");
        assert_eq!(dec("int32", 2, &[0xFFFF, 0xFFFE]), "-2");
        assert_eq!(dec("int32", 2, &[0x8000, 0]), "n/a");
        assert_eq!(dec("uint32", 2, &[1, 0]), "65536");
        assert_eq!(dec("uint32", 2, &[0xFFFF, 0xFFFF]), "n/a");
        assert_eq!(dec("int64", 4, &[0x8000, 0, 0, 0]), "n/a");
        assert_eq!(dec("uint64", 4, &[0, 0, 1, 0]), "65536");
        assert_eq!(dec("uint64", 4, &[0xFFFF; 4]), "n/a");
        // Accumulators have no sentinel of their own here.
        assert_eq!(dec("acc32", 2, &[0xFFFF, 0xFFFF]), "4294967295");
        assert_eq!(dec("acc16", 1, &[0]), "0");
    }

    #[test]
    fn floats_and_nan() {
        assert_eq!(dec("float32", 2, &[0x3FC0, 0]), "1.5");
        assert_eq!(dec("float32", 2, &[0x7FC0, 0]), "n/a");
        assert_eq!(dec("float64", 4, &[0x3FF8, 0, 0, 0]), "1.5");
        assert_eq!(dec("float64", 4, &[0x7FF8, 0, 0, 0]), "n/a");
    }

    #[test]
    fn scale_factors_shift_the_decimal_point_exactly() {
        let p = with_sf(pt("W", "int16", 1), "W_SF");
        let show = |raw: u16, sf: u16| decode(&p, &[raw], Some(scale_factor(sf))).unwrap().to_string();
        assert_eq!(show(4520, 0), "4520");
        assert_eq!(show(4520, 2), "452000");
        assert_eq!(show(4520, 0xFFFF), "452.0");
        assert_eq!(show(4520, 0xFFFE), "45.20");
        assert_eq!(show(5, 0xFFFD), "0.005");
        assert_eq!(show(0xFFFB, 0xFFFF), "-0.5");
        assert_eq!(show(0, 3), "0");
        // An unimplemented or out-of-range scale factor makes the value unusable.
        assert_eq!(show(4520, 0x8000), "n/a");
        assert_eq!(show(4520, 11), "n/a");
        assert_eq!(scale_factor(0xFFF6), Some(-10));
    }

    #[test]
    fn enums_and_bitfields_use_their_symbols() {
        let mut st = pt("St", "enum16", 1);
        st.symbols = vec![sym("OFF", 1), sym("MPPT", 4)];
        assert_eq!(decode(&st, &[4], None).unwrap().to_string(), "MPPT (4)");
        assert_eq!(decode(&st, &[9], None).unwrap().to_string(), "9");
        assert_eq!(decode(&st, &[0xFFFF], None).unwrap().to_string(), "n/a");

        let mut ev = pt("Evt1", "bitfield32", 2);
        ev.symbols = vec![sym("GROUND_FAULT", 0), sym("AC_DISCONNECT", 2), sym("HIGH", 30)];
        assert_eq!(decode(&ev, &[0, 0b101], None).unwrap().to_string(), "GROUND_FAULT, AC_DISCONNECT (0x5)");
        assert_eq!(decode(&ev, &[0x4000, 0], None).unwrap().to_string(), "HIGH (0x40000000)");
        assert_eq!(decode(&ev, &[0, 0], None).unwrap().to_string(), "0x0");
        // The top bit voids the rest.
        assert_eq!(decode(&ev, &[0x8000, 1], None).unwrap().to_string(), "n/a");
        assert_eq!(dec("bitfield16", 1, &[0xFFFF]), "n/a");
    }

    #[test]
    fn strings_drop_trailing_nuls() {
        assert_eq!(dec("string", 4, &[0x4143, 0x4D45, 0, 0]), "ACME");
        assert_eq!(dec("string", 2, &[0x4142, 0x4300]), "ABC");
        assert_eq!(dec("string", 2, &[0, 0]), "n/a");
    }

    #[test]
    fn addresses() {
        assert_eq!(dec("ipaddr", 2, &[0xC0A8, 0x0001]), "192.168.0.1");
        assert_eq!(dec("eui48", 4, &[0, 0x0011, 0x2233, 0x4455]), "00:11:22:33:44:55");
        assert_eq!(dec("ipv6addr", 8, &[0x2001, 0xDB8, 0, 0, 0, 0, 0, 1]), "2001:db8::1");
    }

    #[test]
    fn short_register_slices_are_refused_not_indexed() {
        assert_eq!(decode(&pt("x", "uint32", 2), &[1], None), None);
        assert_eq!(decode(&pt("x", "string", 8), &[0x4142; 3], None), None);
    }

    fn bank(entries: &[(u16, u16)]) -> impl Fn(u16) -> u16 + '_ {
        move |a| entries.iter().find(|(addr, _)| *addr == a).map_or(0, |&(_, v)| v)
    }

    #[test]
    fn discovery_walks_the_models_from_each_base() {
        for base in BASE_ADDRESSES {
            let at = |o: u16| base + o;
            let regs = [
                (at(0), 0x5375),
                (at(1), 0x6E53),
                (at(2), 1),
                (at(3), 66),
                (at(70), 103),
                (at(71), 50),
                (at(122), 0xFFFF),
            ];
            let dev = discover(bank(&regs)).unwrap();
            assert_eq!(dev.base, base);
            assert!(dev.complete);
            assert_eq!(
                dev.models,
                [ModelHeader { id: 1, addr: at(2), len: 66 }, ModelHeader { id: 103, addr: at(70), len: 50 }]
            );
        }
        assert_eq!(discover(|_| 0), None);
    }

    #[test]
    fn discovery_stops_at_the_end_of_the_bank() {
        let regs = [(50000, 0x5375), (50001, 0x6E53), (50002, 7), (50003, 0xFFF0)];
        let dev = discover(bank(&regs)).unwrap();
        assert_eq!(dev.models, [ModelHeader { id: 7, addr: 50002, len: 0xFFF0 }]);
        assert!(!dev.complete);
    }

    fn model() -> Model {
        let header = vec![pt("ID", "uint16", 1), pt("L", "uint16", 1)];
        let mut points = header;
        points.push(with_sf(pt("DCA", "uint16", 1), "DCA_SF"));
        points.push(pt("DCA_SF", "sunssf", 1));
        let module = Group {
            name: "module".into(),
            count: GroupCount::FromLength,
            points: vec![pt("ID", "uint16", 1), with_sf(pt("DCA", "uint16", 1), "DCA_SF")],
            groups: Vec::new(),
        };
        Model {
            id: 64000,
            label: None,
            group: Group { name: "m".into(), count: GroupCount::Fixed(1), points, groups: vec![module] },
        }
    }

    #[test]
    fn layout_expands_repeating_groups_from_the_length() {
        let m = model();
        // Fixed part: DCA, DCA_SF = 2 registers; three modules of 2 = 6; L = 8.
        let placed = layout(&m, ModelHeader { id: 64000, addr: 100, len: 8 });
        let names: Vec<_> = placed.iter().map(|p| (p.path.as_str(), p.addr)).collect();
        assert_eq!(
            names,
            [
                ("ID", 100),
                ("L", 101),
                ("DCA", 102),
                ("DCA_SF", 103),
                ("module[1].ID", 104),
                ("module[1].DCA", 105),
                ("module[2].ID", 106),
                ("module[2].DCA", 107),
                ("module[3].ID", 108),
                ("module[3].DCA", 109),
            ]
        );
        // The modules' DCA uses the scale factor of the enclosing group.
        assert!(placed.iter().filter(|p| p.point.name == "DCA").all(|p| p.sf == Some(ResolvedSf::At(103))));
    }

    #[test]
    fn layout_honours_fixed_counts_and_the_model_length() {
        let mut m = model();
        m.group.groups[0].count = GroupCount::Fixed(1);
        let placed = layout(&m, ModelHeader { id: 64000, addr: 0, len: 8 });
        assert_eq!(placed.last().unwrap().path, "module[1].DCA");
        // A length shorter than the fixed part cuts the points off.
        let short = layout(&m, ModelHeader { id: 64000, addr: 0, len: 1 });
        assert_eq!(short.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["ID", "L", "DCA"]);
        // A missing scale factor point is reported, not guessed.
        m.group.points[2].sf = Some(ScaleFactor::Point("NOPE".into()));
        assert_eq!(layout(&m, ModelHeader { id: 64000, addr: 0, len: 8 })[2].sf, Some(ResolvedSf::Missing));
        // A model near the top of the bank never wraps.
        assert!(layout(&m, ModelHeader { id: 64000, addr: 0xFFFE, len: 8 }).iter().all(|p| p.addr >= 0xFFFE));
    }

    #[test]
    fn decimals_format_exactly() {
        let d = |raw, exp| Decimal { raw, exp }.to_string();
        assert_eq!(d(12345, -2), "123.45");
        assert_eq!(d(-12345, -2), "-123.45");
        assert_eq!(d(7, -3), "0.007");
        assert_eq!(d(7, 3), "7000");
        assert_eq!(d(i128::from(u64::MAX), -10), "1844674407.3709551615");
    }
}
