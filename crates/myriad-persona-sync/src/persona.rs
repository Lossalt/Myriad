//! Her persona as it crosses: one entity, field by field, each field with
//! its own revision so the latest edit of each wins and the value it
//! replaced is kept.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    SyncError,
    entity::Picture,
    hlc::{Rev, check_node},
};

/// How a field merges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldClass {
    /// One value; the later revision wins and the other is kept in history.
    Single,
    /// A list of strings; both sides' items are kept.
    List,
    /// A key only some side gives a meaning to: the site's `displayName`
    /// and `draftSource`, and whatever else a side keeps that the others do
    /// not read. Every side keeps it and sends it back exactly as it came;
    /// it merges like a single value. A comparison leaves it out: a side
    /// that does not use a key has no view of it to differ on.
    Opaque,
}

/// Fields with a known meaning. Her `visual.*` keys are single values of
/// any shape; every other field is [`FieldClass::Opaque`].
pub const KNOWN_FIELDS: &[(&str, FieldClass)] = &[
    ("drives", FieldClass::List),
    ("language", FieldClass::Single),
    ("likes", FieldClass::List),
    ("name", FieldClass::Single),
    ("personality", FieldClass::Single),
    ("socialStyle", FieldClass::Single),
    ("speechStyle", FieldClass::Single),
    ("stickerAvatar", FieldClass::Single),
    ("summary", FieldClass::Single),
    ("temperament", FieldClass::List),
    ("zone", FieldClass::Single),
];

/// Fields that hold text, or nothing.
const TEXT_FIELDS: &[&str] = &[
    "language",
    "name",
    "personality",
    "socialStyle",
    "speechStyle",
    "summary",
    "zone",
];

/// Her visual profile's keys cross under this prefix (`visual.clothingStyle`).
/// The wardrobe crosses as outfits, and which outfit she wears is each
/// side's own.
pub const VISUAL_PREFIX: &str = "visual.";
/// Longest field name.
pub const MAX_FIELD_CHARS: usize = 64;

pub fn field_class(field: &str) -> FieldClass {
    KNOWN_FIELDS
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, class)| *class)
        .unwrap_or(if field.starts_with(VISUAL_PREFIX) {
            FieldClass::Single
        } else {
            FieldClass::Opaque
        })
}

/// One field: its value, and the revision and side of its last edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaField {
    /// `null` is a field cleared.
    pub value: Value,
    pub rev: Rev,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaBody {
    pub fields: BTreeMap<String, PersonaField>,
}

impl PersonaBody {
    /// Her name, trimmed; none when unset or empty.
    pub fn name(&self) -> Option<&str> {
        self.fields
            .get("name")
            .and_then(|field| field.value.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }
}

impl crate::entity::CheckBody for PersonaBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        for (name, field) in &mut self.fields {
            if name.is_empty()
                || name.chars().count() > MAX_FIELD_CHARS
                || name.chars().any(char::is_control)
            {
                return Err(SyncError::new(format!("field name {name:?}")));
            }
            check_node(&field.origin)?;
            let fine = match &field.value {
                Value::Null => true,
                value if field_class(name) == FieldClass::List => value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string)),
                value if name == "stickerAvatar" => {
                    let mut picture: Picture = serde_json::from_value(value.clone())
                        .map_err(|error| SyncError::new(format!("stickerAvatar: {error}")))?;
                    picture.check()?;
                    field.value = serde_json::to_value(picture).map_err(SyncError::from_json)?;
                    true
                }
                Value::String(_) => true,
                _ => !TEXT_FIELDS.contains(&name.as_str()),
            };
            if !fine {
                return Err(SyncError::new(format!(
                    "field {name:?} has the wrong shape"
                )));
            }
        }
        Ok(())
    }
}
