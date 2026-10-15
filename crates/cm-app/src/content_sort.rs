//! Ordering of filtered content indices; source indices (and selections) stay stable.

use std::cmp::Ordering;

use ac_core::content::{Car, TrackEntry, natural_cmp};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum CarSort {
    #[default]
    Name,
    Brand,
    Class,
    Country,
    Year,
    Power,
    Torque,
    Weight,
    PowerWeight,
    TopSpeed,
    Acceleration,
    Skins,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TrackSort {
    #[default]
    Name,
    Country,
    City,
    Length,
    PitBoxes,
}

fn directed(order: Ordering, descending: bool) -> Ordering {
    if descending { order.reverse() } else { order }
}

// Missing metadata always follows known values, in either direction.
fn optional<T>(
    a: Option<T>,
    b: Option<T>,
    descending: bool,
    cmp: impl FnOnce(T, T) -> Ordering,
) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => directed(cmp(a, b), descending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn text<'a>(a: Option<&'a str>, b: Option<&'a str>, descending: bool) -> Ordering {
    let known = |s: &&str| !s.trim().is_empty() && !matches!(s.trim(), "--" | "?");
    optional(a.filter(known), b.filter(known), descending, natural_cmp)
}

/// Read a leading numeric quantity, accepting decimal commas and grouped thousands.
/// A bare dot is decimal here; track lengths additionally handle AC's grouped metres.
fn quantity(raw: &str) -> Option<(f64, String)> {
    let raw = raw.trim().trim_start_matches(['~', '≈', '>']).trim_start();
    let end = raw
        .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | ',' | ' ' | '\u{a0}' | '\''))
        .unwrap_or(raw.len());
    let mut number: String = raw[..end]
        .chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\''))
        .collect();
    if number.contains(',') && number.contains('.') {
        if number.rfind(',') > number.rfind('.') {
            number = number.replace('.', "").replace(',', ".");
        } else {
            number = number.replace(',', "");
        }
    } else if number.contains(',') {
        let parts: Vec<_> = number.split(',').collect();
        if parts.len() > 1 && parts.iter().skip(1).all(|p| p.len() == 3) {
            number = number.replace(',', "");
        } else {
            number = number.replace(',', ".");
        }
    }
    let value: f64 = number.parse().ok()?;
    (value.is_finite() && value > 0.).then(|| (value, raw[end..].trim().to_lowercase()))
}

fn spec(car: &Car, key: &str) -> Option<f64> {
    let raw = &car.specs.iter().find(|(k, _)| k == key)?.1;
    let (value, unit) = quantity(raw)?;
    let factor = match key {
        "bhp" if unit.starts_with("kw") => 1.34102209,
        "bhp" if unit.starts_with("ps") || unit.starts_with("cv") => 0.98632007,
        "torque" if unit.starts_with("lb") => 1.35581795,
        "weight" if unit.starts_with("lb") => 0.45359237,
        "weight" if unit == "t" || unit.starts_with("ton") => 1000.,
        "topspeed" if unit.starts_with("mph") => 1.609344,
        _ => 1.,
    };
    Some(value * factor)
}

fn length(track: &TrackEntry) -> Option<f64> {
    let raw = track.layout.length.as_deref()?.trim();
    let (value, unit) = quantity(raw)?;
    // Stock Laguna Seca stores "3.602" for 3602 metres, unlike "3.602 km".
    if unit.is_empty() && raw.contains('.') && !raw.contains(',') {
        let groups: Vec<_> = raw.split('.').collect();
        if (1..=3).contains(&groups[0].len()) && groups.iter().skip(1).all(|group| group.len() == 3)
        {
            return quantity(&raw.replace('.', "")).map(|(value, _)| value);
        }
    }
    Some(
        value
            * if unit.starts_with("km") {
                1000.
            } else if unit.starts_with("mi") {
                1609.344
            } else {
                1.
            },
    )
}

impl CarSort {
    pub const ALL: [Self; 12] = [
        Self::Name,
        Self::Brand,
        Self::Class,
        Self::Country,
        Self::Year,
        Self::Power,
        Self::Torque,
        Self::Weight,
        Self::PowerWeight,
        Self::TopSpeed,
        Self::Acceleration,
        Self::Skins,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Brand => "Brand",
            Self::Class => "Class",
            Self::Country => "Country",
            Self::Year => "Year",
            Self::Power => "Power (hp)",
            Self::Torque => "Torque (Nm)",
            Self::Weight => "Weight (kg)",
            Self::PowerWeight => "Power/weight (hp/t)",
            Self::TopSpeed => "Top speed",
            Self::Acceleration => "0–100 time",
            Self::Skins => "Skin count",
        }
    }

    pub fn compare(self, a: &Car, b: &Car, descending: bool) -> Ordering {
        let numeric = |car: &Car| match self {
            Self::Year => car.year.as_deref().and_then(quantity).map(|(v, _)| v),
            Self::Power => spec(car, "bhp"),
            Self::Torque => spec(car, "torque"),
            Self::Weight => spec(car, "weight"),
            Self::PowerWeight => Some(spec(car, "bhp")? / spec(car, "weight")? * 1000.),
            Self::TopSpeed => spec(car, "topspeed"),
            Self::Acceleration => spec(car, "acceleration"),
            Self::Skins => Some(car.skins.len() as f64),
            _ => None,
        };
        let order = match self {
            Self::Name => directed(natural_cmp(&a.name, &b.name), descending),
            Self::Brand => text(a.brand.as_deref(), b.brand.as_deref(), descending),
            Self::Class => text(a.class.as_deref(), b.class.as_deref(), descending),
            Self::Country => text(a.country.as_deref(), b.country.as_deref(), descending),
            _ => optional(numeric(a), numeric(b), descending, |a, b| a.total_cmp(&b)),
        };
        order
            .then_with(|| natural_cmp(&a.name, &b.name))
            .then_with(|| a.id.cmp(&b.id))
    }
}

impl TrackSort {
    pub const ALL: [Self; 5] = [
        Self::Name,
        Self::Country,
        Self::City,
        Self::Length,
        Self::PitBoxes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Country => "Country",
            Self::City => "City",
            Self::Length => "Distance",
            Self::PitBoxes => "Pit boxes",
        }
    }

    pub fn compare(self, a: &TrackEntry, b: &TrackEntry, descending: bool) -> Ordering {
        let order = match self {
            Self::Name => directed(natural_cmp(&a.layout.name, &b.layout.name), descending),
            Self::Country => text(
                a.layout.country.as_deref(),
                b.layout.country.as_deref(),
                descending,
            ),
            Self::City => text(
                a.layout.city.as_deref(),
                b.layout.city.as_deref(),
                descending,
            ),
            Self::Length => optional(length(a), length(b), descending, |a, b| a.total_cmp(&b)),
            Self::PitBoxes => optional(a.layout.pitboxes, b.layout.pitboxes, descending, |a, b| {
                a.cmp(&b)
            }),
        };
        order
            .then_with(|| natural_cmp(&a.layout.name, &b.layout.name))
            .then_with(|| a.key().cmp(&b.key()))
    }
}
