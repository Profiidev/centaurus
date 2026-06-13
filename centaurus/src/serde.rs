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

#[cfg(test)]
mod tests {
  use super::*;
  use serde::Serialize;

  #[test]
  fn test_empty_string_as_none() {
    #[derive(Deserialize)]
    struct Test {
      #[serde(deserialize_with = "empty_string_as_none")]
      val: Option<i32>,
    }

    let t: Test = serde_json::from_str(r#"{"val": ""}"#).unwrap();
    assert_eq!(t.val, None);

    let t: Test = serde_json::from_str(r#"{"val": "123"}"#).unwrap();
    assert_eq!(t.val, Some(123));

    let t: Test = serde_json::from_str(r#"{"val": null}"#).unwrap();
    assert_eq!(t.val, None);
  }

  #[test]
  fn test_de_str() {
    #[derive(Deserialize)]
    struct Test {
      #[serde(deserialize_with = "de_str")]
      val: i32,
    }

    let t: Test = serde_json::from_str(r#"{"val": "123"}"#).unwrap();
    assert_eq!(t.val, 123);
  }

  #[test]
  fn test_se_str() {
    #[derive(Serialize)]
    struct Test {
      #[serde(serialize_with = "se_str")]
      val: i32,
    }

    let t = Test { val: 123 };
    let s = serde_json::to_string(&t).unwrap();
    assert_eq!(s, r#"{"val":"123"}"#);
  }
}
