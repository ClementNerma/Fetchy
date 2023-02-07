use pomsky::{diagnose::Severity, options::CompileOptions};
use regex::Regex;
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::{error, warn};

pub struct Pattern {
    pub source: String,
    pub regex: Regex,
    pub captures: bool,
}

impl Serialize for Pattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.source)
    }
}

struct PomskyRegexVisitor;

impl<'de> Visitor<'de> for PomskyRegexVisitor {
    type Value = Pattern;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(" a Pomsky regex")
    }

    fn visit_str<E>(self, str: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let (expr, warnings) = pomsky::Expr::parse(str);

        for warn in warnings {
            match warn.severity {
                Severity::Error => error!("Failed to parse Pomsky regex: {}", warn.msg),
                Severity::Warning => warn!("Pomsky emitted a warning: {}", warn.msg),
            }
        }

        let expr = expr.ok_or_else(|| E::custom("Regex could not be parsed"))?;

        let compiled = expr
            .compile(str, CompileOptions::default())
            .map_err(|err| {
                E::custom(format!(
                    "Failed to compile provided regex ({str}): {}",
                    err.msg
                ))
            })?;

        let regex = Regex::new(&compiled).map_err(|err| {
            E::custom(format!(
                "Failed to parse compiled regex ({compiled}): {err}"
            ))
        })?;

        if regex.captures_len() > 1 {
            E::custom(&format!(
                "Regex ({}) is only allowed to have one single capture group",
                str
            ));
        }

        Ok(Pattern {
            source: str.to_owned(),
            captures: regex.captures_len() == 1,
            regex,
        })
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(PomskyRegexVisitor)
    }
}
