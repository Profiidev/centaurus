use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, de};

pub fn empty_string_as_none<'de, D, T>(de: D) -> std::result::Result<Option<T>, D::Error>
where
  D: Deserializer<'de>,
  T: FromStr,
  T::Err: fmt::Display,
{
  let opt = Option::<String>::deserialize(de)?;
  match opt.as_deref() {
    None | Some("") => Ok(None),
    Some(s) => FromStr::from_str(s).map_err(de::Error::custom).map(Some),
  }
}

pub fn de_str<'de, D: Deserializer<'de>, T: FromStr>(d: D) -> Result<T, D::Error>
where
  T::Err: std::fmt::Display,
{
  let s = String::deserialize(d)?;
  T::from_str(&s).map_err(serde::de::Error::custom)
}

pub fn se_str<T: ToString, S: serde::Serializer>(t: &T, s: S) -> Result<S::Ok, S::Error> {
  s.serialize_str(&t.to_string())
}
